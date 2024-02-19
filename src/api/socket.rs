use std::sync::Arc;

use futures::FutureExt;
use futures_util::stream::StreamExt;
use futures_util::{pin_mut, sink::SinkExt};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::api::{authenticate, AuthenticatedUser};
use subseq_util::oidc::IdentityProvider;
use subseq_util::Router;
use tokio::{sync::mpsc, task::spawn};
use warp::filters::ws::{Message, WebSocket};
use warp::{Filter, Rejection, Reply};
use warp_sessions::{MemoryStore, SessionWithStore};

use super::prompts::{ChatCompletion, InstructChannel};
use super::voice::{create_audio_timing_task, AudioContext, AudioData, AudioEventChannel};

const WEBSOCKET_BUFFER_SIZE: usize = 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WebSocketMessage {
    Instruct(String),
    SetAudioContext(AudioContext),
}

#[derive(Clone, Debug)]
pub enum FrontEndMessage {
    InstructMessage(ChatCompletion),
    InstructClear,
    SetProject(String),
}

impl Serialize for FrontEndMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let state = match self {
            FrontEndMessage::InstructClear => {
                let mut state = serializer.serialize_struct("FrontEndMessage", 1)?;
                state.serialize_field("channel", "INSTRUCT-CLEAR")?;
                state
            }
            FrontEndMessage::SetProject(project) => {
                let mut state = serializer.serialize_struct("FrontEndMessage", 2)?;
                state.serialize_field("channel", "PROJECT-SET")?;
                state.serialize_field("project", &project)?;
                state
            }
            FrontEndMessage::InstructMessage(completion) => {
                let mut state = serializer.serialize_struct("FrontEndMessage", 3)?;
                state.serialize_field("channel", "INSTRUCT-MESSAGE")?;
                state.serialize_field("role", &completion.role)?;
                state.serialize_field("content", &completion.content)?;
                state
            }
        };
        state.end()
    }
}

async fn client_websocket(
    auth_user: AuthenticatedUser,
    ws: WebSocket,
    audio_event: mpsc::Sender<AudioEventChannel>,
    instruct_config_tx: mpsc::Sender<InstructChannel>,
) {
    let user_id = auth_user.id();
    tracing::info!("Client {} ws started", user_id);
    let (mut write, mut read) = ws.split();

    let (output_tx, mut output_rx) = mpsc::channel::<FrontEndMessage>(WEBSOCKET_BUFFER_SIZE);
    let (instruct_out_tx, mut instruct_out_rx) = mpsc::channel(WEBSOCKET_BUFFER_SIZE);
    let (instruct_in_tx, instruct_in_rx) = mpsc::channel(WEBSOCKET_BUFFER_SIZE);
    let (audio_tx, audio_rx) = mpsc::channel(WEBSOCKET_BUFFER_SIZE);

    if instruct_config_tx
        .send((auth_user, instruct_in_rx, instruct_out_tx))
        .await
        .is_err()
    {
        tracing::error!("Could not set up instruct streams for web socket");
        return;
    }

    let (audio_text_tx, mut audio_text_rx) = mpsc::channel(WEBSOCKET_BUFFER_SIZE);
    create_audio_timing_task(audio_text_tx, audio_rx, audio_event);

    spawn(async move {
        while let Some(chat) = instruct_out_rx.recv().await {
            if output_tx
                .send(FrontEndMessage::InstructMessage(chat))
                .await
                .is_err()
            {
                break;
            }
        }
        tracing::info!("{} instruct out handler exited", user_id);
    });

    // TODO: handle context -> send message to context to fill in text for feedback
    // TODO: handle streaming messages -> check finalized bit
    let instruct_tx = instruct_in_tx.clone();
    spawn(async move {
        while let Some(audio_response) = audio_text_rx.recv().await {
            instruct_tx.send(audio_response.payload).await.ok();
        }
        tracing::info!("{} audio text handler exited", user_id);
    });

    let write_handler = spawn(async move {
        while let Some(message) = output_rx.recv().await {
            let serialized = serde_json::to_string(&message).unwrap();
            write.send(Message::text(serialized)).await.ok();
        }
        tracing::info!("{} write handler exited", user_id);
    })
    .fuse();

    let read_handler = spawn(async move {
        let mut msg_counter = 0;
        let mut context = AudioContext::Discover;

        while let Some(message) = read.next().await {
            if let Ok(message) = message {
                if message.is_text() {
                    let text = message.to_str().unwrap();
                    let decoded: WebSocketMessage = match serde_json::from_str(text) {
                        Ok(decoded) => decoded,
                        Err(_) => {
                            tracing::warn!("Invalid message received: {}", text);
                            continue;
                        }
                    };
                    match decoded {
                        WebSocketMessage::Instruct(msg) => {
                            tracing::info!("Instruct: {}", text);
                            if instruct_in_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                        WebSocketMessage::SetAudioContext(new_context) => {
                            context = new_context;
                            msg_counter = 0;
                        }
                    }
                } else if message.is_binary() {
                    let audio = AudioData {
                        payload: message.as_bytes().to_vec(),
                        count: msg_counter,
                    };
                    audio_tx.send((context, audio)).await.ok();
                    msg_counter += 1;
                } else if message.is_close() {
                    break;
                } else {
                    tracing::warn!("Unexpected ws message from {}", user_id);
                }
            } else {
                break;
            }
        }
        tracing::info!("{} read handler exited", user_id);
    })
    .fuse();

    pin_mut!(write_handler, read_handler);

    // If either task exits we want to clean up.
    futures::select!(
        _ = write_handler => {},
        _ = read_handler => {},
    );
    tracing::info!("Client {} ws closed", user_id);
}

pub fn routes(
    idp: Option<Arc<IdentityProvider>>,
    session: MemoryStore,
    router: &mut Router,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let audio_stream: mpsc::Sender<AudioEventChannel> = router
        .get_address()
        .expect("Could not get AudioDataChannel")
        .clone();
    let instruction_config_tx: mpsc::Sender<InstructChannel> = router
        .get_address()
        .expect("Could not get InstructChannel")
        .clone();

    warp::path("ws")
        .and(authenticate(idp, session.clone()))
        .and(warp::ws())
        .map(
            move |auth: AuthenticatedUser,
                  session: SessionWithStore<MemoryStore>,
                  ws: warp::ws::Ws| {
                let audio_stream = audio_stream.clone();
                let instruction_config_tx = instruction_config_tx.clone();
                (
                    ws.on_upgrade(move |socket| {
                        client_websocket(auth, socket, audio_stream, instruction_config_tx)
                    }),
                    session,
                )
            },
        )
        .untuple_one()
        .and_then(store_auth_cookie)
}
