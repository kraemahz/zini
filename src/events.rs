use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Future;
use http::Uri;
use prism_client::{AsyncClient, Wavelet};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use subseq_util::router::Router;
use subseq_util::tables::{DbPool, UserTable};
use tokio::spawn;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::api::prompts::{InitializePromptChannel, PromptTx, PromptRx};
use crate::api::voice::{
    SpeechToText,
    SpeechToTextResponse,
    SpeechToTextRequest,
    VoiceResponseCollection
};
use crate::{
    interop::{Job, JobResult},
    tables::{Project, Task, Flow, Graph, User},
    api::tasks::TaskStatePayload,
    api::prompts::PromptResponseCollection
};


pub fn prism_url(host: &str, port: u16) -> String {
    format!("ws://{}:{}", host, port)
}

const USER_CREATED_BEAM: &str = "urn:subseq.io:oidc:user:created";
const USER_UPDATED_BEAM: &str = "urn:subseq.io:oidc:user:updated";

const JOB_RESULT_BEAM: &str = "urn:subseq.io:builds:job:result";
const JOB_CREATED_BEAM: &str = "urn:subseq.io:builds:k8s:job:created";

// Task Broadcast
const TASK_CREATED_BEAM: &str = "urn:subseq.io:tasks:task:created";
const TASK_RUN_BEAM: &str = "urn:subseq.io:tasks:task:run";
const TASK_UPDATED_BEAM: &str = "urn:subseq.io:tasks:task:updated";
const TASK_ASSIGNEE_BEAM: &str = "urn:subseq.io:tasks:task:assignee:changed";
const TASK_STATE_BEAM: &str = "urn:subseq.io:tasks:task:state:changed";

const PROJECT_CREATED_BEAM: &str = "urn:subseq.io:tasts:project:created";
const PROJECT_UPDATED_BEAM: &str = "urn:subseq.io:tasks:project:updated";

const FLOW_CREATED_BEAM: &str = "urn:subseq.io:tasks:workflow:created";
const FLOW_UPDATED_BEAM: &str = "urn:subseq.io:tasks:workflow:updated";


async fn setup_user_beams(client: &mut AsyncClient) {
    client.add_beam(USER_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(USER_UPDATED_BEAM).await.expect("Failed setting up client");

    client.subscribe(USER_CREATED_BEAM, None).await.expect("Failed subscribing");
    client.subscribe(USER_UPDATED_BEAM, None).await.expect("Failed subscribing");
}


async fn setup_job_beams(client: &mut AsyncClient) {
    client.subscribe(JOB_RESULT_BEAM, None).await.expect("Failed subscribing");
    client.subscribe(JOB_CREATED_BEAM, None).await.expect("Failed subscribing");
}


async fn setup_task_beams(client: &mut AsyncClient) {
    client.add_beam(TASK_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(TASK_UPDATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(TASK_ASSIGNEE_BEAM).await.expect("Failed setting up client");
    client.add_beam(TASK_STATE_BEAM).await.expect("Failed setting up client");
}

async fn setup_project_beams(client: &mut AsyncClient) {
    client.add_beam(PROJECT_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(PROJECT_UPDATED_BEAM).await.expect("Failed setting up client");
}


async fn setup_flow_beams(client: &mut AsyncClient) {
    client.add_beam(FLOW_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(FLOW_UPDATED_BEAM).await.expect("Failed setting up client");
}


fn gen_rand() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

fn gen_voice_beams() -> (String, String) {
    let voice_stream: &str = "urn:subseq.io:voice:audio:request";
    let text_stream: String = format!("urn:subseq.io:voice:text:{}", gen_rand());

    (voice_stream.to_string(), text_stream)
}

fn gen_prompt_beams() -> (String, String) {
    let prompt_tx_stream: &str = "urn:subseq.io:prompt:stream:input";
    let prompt_rx_stream: String = format!("urn:subseq.io:prompt:stream:{}", gen_rand());
    (prompt_tx_stream.to_string(), prompt_rx_stream)
}

#[derive(Clone)]
pub struct UserCreated(pub User);

pub fn create_users_from_events(mut user_created_rx: broadcast::Receiver<UserCreated>,
                                db_pool: Arc<DbPool>) {
    spawn(async move {
        while let Ok(created_user) = user_created_rx.recv().await {
            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => return
            };
            let user = created_user.0;
            if User::get(&mut conn, user.id).is_none() {
                User::create(&mut conn, user.id, &user.email, None).ok();
            }
        }
    });
}

#[derive(Serialize)]
struct PromptTxWrapper {
    prompt_tx: PromptTx,
    beam: String
}

struct WaveletHandler {
    job_result_tx: broadcast::Sender<JobResult>,
    job_created_tx: broadcast::Sender<Job>,
    user_created_tx: broadcast::Sender<UserCreated>,
    prompt_requests: PromptResponseCollection,
    prompt_beam: String,
    voice_requests: VoiceResponseCollection,
    voice_beam: String,
    wavelet: Wavelet,
}

impl Future for WaveletHandler {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Wavelet { beam, photons } = &this.wavelet;
        match beam.as_str() { 
            JOB_RESULT_BEAM => {
                for photon in photons {
                    let result: JobResult = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", JOB_RESULT_BEAM);
                            continue;
                        }
                    };
                    this.job_result_tx.send(result).ok();
                }
            }
            JOB_CREATED_BEAM => {
                for photon in photons {
                    let result: Job = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", JOB_RESULT_BEAM);
                            continue;
                        }
                    };
                    this.job_created_tx.send(result).ok();
                }
            }
            USER_CREATED_BEAM => {
                for photon in photons {
                    let result: User = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", JOB_RESULT_BEAM);
                            continue;
                        }
                    };
                    this.user_created_tx.send(UserCreated(result)).ok();
                }
            }
            b => {
                if b == this.voice_beam {
                    for photon in photons {
                        let result: SpeechToTextResponse = match serde_cbor::from_slice(&photon.payload) {
                            Ok(ok) => ok,
                            Err(_) => {
                                tracing::error!("Received invalid Photon on {}", JOB_RESULT_BEAM);
                                continue;
                            }
                        };
                        this.voice_requests.send_response(result);
                    }
                } else if b == this.prompt_beam {
                    for photon in photons {
                        let result: PromptRx = match serde_json::from_slice(&photon.payload) {
                            Ok(ok) => ok,
                            Err(_) => {
                                tracing::error!("Received invalid Photon on {}", this.prompt_beam);
                                continue;
                            }
                        };
                        this.prompt_requests.send_response(result);
                    }
                } else {
                    tracing::error!("Received unhandled Beam: {}", b);
                }
            }
        }

        Poll::Ready(())
    }
}


pub fn emit_events(addr: &str, router: &mut Router, db_pool: Arc<DbPool>) {
    // Tables
    let mut user_rx: broadcast::Receiver<User> = router.subscribe();
    let user_created_rx: broadcast::Receiver<UserCreated> = router.subscribe();
    create_users_from_events(user_created_rx, db_pool);
    let mut task_rx: broadcast::Receiver<Task> = router.subscribe();
    let mut task_update_rx: broadcast::Receiver<TaskStatePayload> = router.subscribe();
    let mut project_rx: broadcast::Receiver<Project> = router.subscribe();
    let mut flow_rx: broadcast::Receiver<Flow> = router.subscribe();
    let mut graph_rx: broadcast::Receiver<Graph> = router.subscribe();

    // Voice
    let mut voice_rx: mpsc::Receiver<(SpeechToText, oneshot::Sender<SpeechToTextResponse>)> = router.create_channel();
    let voice_requests = VoiceResponseCollection::new();
    let handler_voice_requests = voice_requests.clone();

    // Jobs
    let job_tx: broadcast::Sender<Job> = router.announce();
    let job_result_tx: broadcast::Sender<JobResult> = router.announce();
    let user_created_tx: broadcast::Sender<UserCreated> = router.announce();

    // Prompts
    let (prompt_request_tx, mut prompt_request_rx) = mpsc::channel::<PromptTxWrapper>(1024);
    let mut new_prompt_rx: mpsc::Receiver<InitializePromptChannel> = router.create_channel();

    let prompt_requests = PromptResponseCollection::new();
    let handler_requests = prompt_requests.clone();

    let uri = addr.parse::<Uri>().unwrap();

    spawn(async move {
        let (voice_request_beam, voice_response_beam) = gen_voice_beams();
        let (prompt_request_beam, prompt_response_beam) = gen_prompt_beams();

        let voice_beam = voice_response_beam.clone();
        let prompt_beam = prompt_response_beam.clone();

        let handle_tasks = move |wavelet: Wavelet| {
            WaveletHandler {
                job_result_tx: job_result_tx.clone(),
                job_created_tx: job_tx.clone(),
                user_created_tx: user_created_tx.clone(),
                prompt_requests: handler_requests.clone(),
                prompt_beam: prompt_beam.clone(),
                voice_requests: handler_voice_requests.clone(),
                voice_beam: voice_beam.clone(),
                wavelet
            }
        };

        let mut client = match AsyncClient::connect(uri, handle_tasks).await {
            Ok(client) => client,
            Err(_err) => {
                tracing::warn!("Zini is running in standalone mode. No connection to prism.");
                return;
            }
        };
        tracing::info!("Zini connected to prism!");

        client.add_beam(&voice_request_beam).await.expect("Failed setting up client");
        client.subscribe(&voice_response_beam, None).await.expect("Failed subscribing");
        client.add_beam(&prompt_request_beam).await.expect("Failed setting up client");
        client.subscribe(&prompt_response_beam, None).await.expect("Failed subscribing");

        setup_user_beams(&mut client).await;
        setup_job_beams(&mut client).await;
        setup_task_beams(&mut client).await;
        setup_project_beams(&mut client).await;
        setup_flow_beams(&mut client).await;

        loop {
            tokio::select!(
                msg = voice_rx.recv() => {
                    if let Some((msg, tx)) = msg {
                        voice_requests.insert(msg.conversation_id, msg.count, tx);
                        let request = SpeechToTextRequest::extend_from(msg, voice_response_beam.clone());
                        let vec = serde_cbor::to_vec(&request).unwrap();
                        if client.emit(&voice_request_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = new_prompt_rx.recv() => {
                    if let Some((stream_id, mut rx, tx)) = msg {
                        prompt_requests.insert(stream_id, tx);
                        let msg_tx = prompt_request_tx.clone();
                        let prompt_response_beam = prompt_response_beam.clone();
                        spawn(async move {
                            while let Some(msg) = rx.recv().await {
                                let wrapper = PromptTxWrapper{prompt_tx: msg, beam: prompt_response_beam.clone()};
                                msg_tx.send(wrapper).await.ok();
                            }
                        });
                    }
                }
                msg = prompt_request_rx.recv() => {
                    if let Some(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(prompt_request_beam.clone(), vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = user_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(USER_CREATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = task_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(TASK_CREATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = task_update_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(TASK_UPDATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = project_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(PROJECT_CREATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = flow_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(FLOW_CREATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = graph_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(FLOW_UPDATED_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
            );
        }
        tracing::warn!("Prism client closed");
    });
}
