use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    task::spawn,
    time::timeout,
};
use uuid::Uuid;

use super::prompts::ChatCompletion;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AudioContext {
    Discover,
    Search,
    Instruct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioContextResponse {
    Search(String),
    Instruct(ChatCompletion),
}
pub type AudioDataChannel = (AudioContext, AudioData);
pub type AudioEventChannel = (SpeechToText, oneshot::Sender<SpeechToTextResponse>);

#[derive(Clone, Debug)]
pub struct AudioData {
    pub payload: Vec<u8>,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct SpeechToText {
    pub conversation_id: Uuid,
    pub payload: Vec<u8>,
    pub count: usize,
    pub finalize: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeechToTextRequest {
    pub conversation_id: Uuid,
    pub payload: Vec<u8>,
    pub count: usize,
    pub beam: String,
    pub finalize: bool,
}

impl SpeechToTextRequest {
    pub fn extend_from(stt: SpeechToText, beam: String) -> SpeechToTextRequest {
        SpeechToTextRequest {
            conversation_id: stt.conversation_id,
            payload: stt.payload,
            count: stt.count,
            beam,
            finalize: stt.finalize,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpeechToTextResponse {
    pub conversation_id: Uuid,
    pub count: usize,
    pub payload: String,
    pub finalized: bool,
}

pub fn create_audio_timing_task(
    text_tx: mpsc::Sender<SpeechToTextResponse>,
    mut audio_rx: mpsc::Receiver<AudioDataChannel>,
    audio_stream: mpsc::Sender<AudioEventChannel>,
) {
    const SPEECH_TIMEOUT: Duration = Duration::from_secs(2);

    spawn(async move {
        let mut n_messages: usize = 0;
        let mut stream: Option<Vec<u8>> = None;
        let mut _context = AudioContext::Discover;

        loop {
            let conversation_id: Uuid = Uuid::new_v4();
            let result = timeout(SPEECH_TIMEOUT, audio_rx.recv()).await;
            let stt = match result {
                Ok(msg) => msg,
                Err(_) => {
                    // Timeout
                    // Collect stream and send as finalized message.
                    if let Some(payload) = stream.take() {
                        tracing::info!("Sending audio stream...");
                        let request = SpeechToText {
                            conversation_id,
                            count: n_messages,
                            payload,
                            finalize: true,
                        };
                        n_messages += 1;
                        let (tx, rx) = oneshot::channel();
                        if audio_stream.send((request, tx)).await.is_err() {
                            break;
                        }

                        let text_tx = text_tx.clone();
                        spawn(async move {
                            if let Ok(response) = rx.await {
                                text_tx.send(response).await.ok();
                            }
                        });
                    }
                    continue;
                }
            };

            if let Some((new_context, AudioData { payload, count: _ })) = stt {
                stream = match stream.take() {
                    Some(mut audio) => {
                        audio.extend(payload);
                        Some(audio)
                    }
                    None => {
                        _context = new_context;
                        Some(payload)
                    }
                };
            } else {
                break;
            }
        }
        tracing::warn!("Speech to instructions exited");
    });
}

type InnerVoiceResponseCollection = HashMap<(Uuid, usize), oneshot::Sender<SpeechToTextResponse>>;

#[derive(Clone, Debug)]
pub struct VoiceResponseCollection {
    inner: Arc<Mutex<InnerVoiceResponseCollection>>,
}

impl VoiceResponseCollection {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert(
        &self,
        conversation_id: Uuid,
        count: usize,
        sender: oneshot::Sender<SpeechToTextResponse>,
    ) {
        self.inner
            .lock()
            .unwrap()
            .insert((conversation_id, count), sender);
    }

    pub fn send_response(&self, voice_response: SpeechToTextResponse) {
        let lookup = &(voice_response.conversation_id, voice_response.count);
        if let Some(sender) = self.inner.lock().unwrap().remove(lookup) {
            sender.send(voice_response).ok();
        }
    }
}

impl Default for VoiceResponseCollection {
    fn default() -> Self {
        Self::new()
    }
}
