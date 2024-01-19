use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use byteorder::{ByteOrder, NetworkEndian};
use futures::FutureExt;
use futures_util::stream::StreamExt;
use futures_util::{pin_mut, sink::SinkExt};
use serde::{Serialize, Deserialize};
use subseq_util::Router;
use subseq_util::api::{authenticate, AuthenticatedUser};
use subseq_util::oidc::IdentityProvider;
use tokio::{sync::{broadcast, mpsc, oneshot}, task::spawn, time::timeout};
use uuid::Uuid;
use warp::filters::ws::{Message, WebSocket};
use warp::{Filter, Reply, Rejection};
use warp_sessions::MemoryStore;

#[derive(Clone, Debug)]
pub struct AudioData {
    pub payload: Vec<i16>,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct SpeechToText{
    pub conversation_id: Uuid,
    pub payload: Vec<i16>,
    pub count: usize,
    pub finalize: bool
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeechToTextRequest {
    pub conversation_id: Uuid,
    pub payload: Vec<i16>,
    pub count: usize,
    pub beam: String,
    pub finalize: bool
}

impl SpeechToTextRequest {
    pub fn extend_from(stt: SpeechToText, beam: String) -> SpeechToTextRequest {
        SpeechToTextRequest {
            conversation_id: stt.conversation_id,
            payload: stt.payload,
            count: stt.count,
            beam,
            finalize: stt.finalize
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpeechToTextResponse {
    pub conversation_id: Uuid,
    pub count: usize,
    pub payload: String,
    pub finalized: bool
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub auth_user: AuthenticatedUser,
    pub instruction: String,
    pub segments: Vec<usize>
}

pub fn create_audio_timing_task(
    auth_user: AuthenticatedUser,
    text_tx: mpsc::Sender<SpeechToTextResponse>,
    mut audio_rx: mpsc::Receiver<AudioData>,
    audio_stream: mpsc::Sender<(SpeechToText, oneshot::Sender<SpeechToTextResponse>)>,
    instruction_tx: broadcast::Sender<Instruction>
) {
    const SPEECH_TIMEOUT: Duration = Duration::from_secs(2);

    spawn(async move {
        let mut n_messages: usize = 0;
        let mut stream: Option<Vec<i16>> = None;
        let mut segments = Vec::new();

        loop {
            let conversation_id: Uuid = Uuid::new_v4();
            let result = timeout(SPEECH_TIMEOUT, audio_rx.recv()).await;
            let stt = match result {
                Ok(msg) => msg,
                Err(_) => {
                    // Timeout
                    // Collect stream and send as finalized message.
                    if let Some(payload) = stream.take() {
                        let request = SpeechToText {
                            conversation_id,
                            count: n_messages,
                            payload,
                            finalize: true
                        };
                        n_messages += 1;
                        // TODO
                        let segments: Vec<_> = segments.drain(..).collect();
                        let (tx, rx) = oneshot::channel();
                        if audio_stream.send((request, tx)).await.is_err() {
                            break;
                        }

                        let text_tx = text_tx.clone();
                        let instruction_tx = instruction_tx.clone();
                        let auth_user = auth_user.clone();
                        spawn(async move {
                            if let Ok(response) = rx.await {
                                let instruction = Instruction {
                                    auth_user,
                                    instruction: response.payload.clone(),
                                    segments
                                };
                                text_tx.send(response.clone()).await.ok();
                                instruction_tx.send(instruction).ok();
                            }
                        });
                    }
                    continue;
                }
            };

            if let Some(AudioData{payload, count}) = stt {
                segments.push(count);

                stream = match stream.take() {
                    Some(mut audio) => {
                        audio.extend(payload);
                        Some(audio)
                    }
                    None => Some(payload)
                };
            }
        }
        tracing::warn!("Speech to instructions exited");
    });
}

const AUDIO_TIMING_BUFFER: usize = 1024;

async fn proxy_audio_socket(
    auth_user: AuthenticatedUser,
    ws: WebSocket,
    audio_stream: mpsc::Sender<(SpeechToText, oneshot::Sender::<SpeechToTextResponse>)>,
    instruction_stream: broadcast::Sender<Instruction>
) {
    let (mut write, mut read) = ws.split();
    let (text_tx, mut text_rx) = mpsc::channel(AUDIO_TIMING_BUFFER);
    let (audio_tx, audio_rx) = mpsc::channel(AUDIO_TIMING_BUFFER);
    create_audio_timing_task(auth_user, text_tx, audio_rx, audio_stream, instruction_stream);

    let write_handler = spawn(async move {
        while let Some(message) = text_rx.recv().await {
            let serialized = serde_json::to_string(&message).unwrap();
            write.send(Message::text(serialized)).await.ok();
        }
        tracing::warn!("Client write socket closed");
    }).fuse();

    let read_handler = spawn(async move {
        let mut msg_counter = 0;
        while let Some(message) = read.next().await {
            if let Ok(message) = message {
                let mut payload = vec![];
                NetworkEndian::read_i16_into(message.as_bytes(), &mut payload);
                let audio = AudioData {
                    payload,
                    count: msg_counter
                };
                audio_tx.send(audio).await.ok();
                msg_counter += 1;
            } else {
                break;
            }
        }
        tracing::warn!("Client read socket closed");
    }).fuse();

    pin_mut!(write_handler, read_handler);

    // If either task exits we want to clean up.
    futures::select!(
        _ = write_handler => {},
        _ = read_handler => {},
    );
}

#[derive(Clone, Debug)]
pub struct VoiceResponseCollection {
    inner: Arc<Mutex<HashMap<(Uuid, usize), oneshot::Sender<SpeechToTextResponse>>>>
}

impl VoiceResponseCollection {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn insert(&self,
                  conversation_id: Uuid,
                  count: usize,
                  sender: oneshot::Sender<SpeechToTextResponse>) {
        self.inner.lock().unwrap().insert((conversation_id, count), sender);
    }

    pub fn send_response(&self, voice_response: SpeechToTextResponse) {
        let lookup = &(voice_response.conversation_id, voice_response.count);
        if let Some(sender) = self.inner.lock().unwrap().remove(lookup) {
            sender.send(voice_response).ok();
        }
    }

}

pub fn routes(
    idp: Arc<IdentityProvider>, session: MemoryStore, router: &mut Router
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let speech_text_tx: mpsc::Sender<(SpeechToText, oneshot::Sender<SpeechToTextResponse>)> =
        router.get_address().expect("Could not get SpeechToText channel").clone();
    let instruction_tx: broadcast::Sender<Instruction> = router.announce();

    let audio_ws = warp::path("audio")
        .and(authenticate(idp, session))
        .and(warp::ws())
        .map(move |auth: AuthenticatedUser, ws: warp::ws::Ws| {
            let speech_text_tx = speech_text_tx.clone();
            let instruction_tx = instruction_tx.clone();
            ws.on_upgrade(move |socket| proxy_audio_socket(auth, socket, speech_text_tx, instruction_tx))
        });
    return audio_ws;
}
