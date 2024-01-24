use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Future;
use http::Uri;
use prism_client::{AsyncClient, Wavelet};
use rand::{distributions::Alphanumeric, Rng};
use subseq_util::router::Router;
use subseq_util::tables::{DbPool, UserTable};
use tokio::spawn;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::api::tasks::{InnerTaskPayload, InnerUpdateTaskPayload};
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
    api::prompts::{PromptRequest, PromptResponse, PromptResponseCollection}
};


pub fn prism_url(host: &str, port: u16) -> String {
    format!("ws://{}:{}", host, port)
}

const USER_CREATED_BEAM: &str = "urn:subseq.io:oidc:user:created";
const USER_UPDATED_BEAM: &str = "urn:subseq.io:oidc:user:updated";

const JOB_RESULT_BEAM: &str = "urn:subseq.io:builds:job:result";
const JOB_CREATED_BEAM: &str = "urn:subseq.io:builds:k8s:job:created";

const CREATE_TASK_BEAM: &str = "urn:subseq.io:tasks:task:create";
const UPDATE_TASK_BEAM: &str = "urn:subseq.io:tasks:task:update";
const TASK_CREATED_BEAM: &str = "urn:subseq.io:tasks:task:created";
const TASK_UPDATED_BEAM: &str = "urn:subseq.io:tasks:task:updated";
const TASK_ASSIGNEE_BEAM: &str = "urn:subseq.io:tasks:task:assignee:changed";
const TASK_STATE_BEAM: &str = "urn:subseq.io:tasks:task:state:changed";

const PROMPT_REQUEST_BEAM: &str = "urn:subseq.io:tasks:prompt:request";
const PROMPT_RESPONSE_BEAM: &str = "urn:subseq.io:tasks:prompt:response";

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

    client.subscribe(CREATE_TASK_BEAM, None).await.expect("Failed subscribing");
    client.subscribe(UPDATE_TASK_BEAM, None).await.expect("Failed subscribing");
}


async fn setup_prompt_beams(client: &mut AsyncClient) {
    client.add_beam(PROMPT_REQUEST_BEAM).await.expect("Failed setting up client");
    client.subscribe(PROMPT_RESPONSE_BEAM, None).await.expect("Failed subscribing");
}


async fn setup_project_beams(client: &mut AsyncClient) {
    client.add_beam(PROJECT_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(PROJECT_UPDATED_BEAM).await.expect("Failed setting up client");
}


async fn setup_flow_beams(client: &mut AsyncClient) {
    client.add_beam(FLOW_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(FLOW_UPDATED_BEAM).await.expect("Failed setting up client");
}

async fn gen_voice_beams() -> (String, String) {
    let rand_id: String = {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect()
    };
    let voice_stream: &str = "urn:subseq.io:voice:audio:request";
    let text_stream: String = format!("urn:subseq.io:voice:text:{}", rand_id);

    (voice_stream.to_string(), text_stream)
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



struct WaveletHandler {
    job_result_tx: broadcast::Sender<JobResult>,
    job_created_tx: broadcast::Sender<Job>,
    user_created_tx: broadcast::Sender<UserCreated>,
    create_task_tx: broadcast::Sender<InnerTaskPayload>,
    update_task_tx: broadcast::Sender<InnerUpdateTaskPayload>,
    prompt_requests: PromptResponseCollection,
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
            PROMPT_RESPONSE_BEAM => {
                for photon in photons {
                    let result: PromptResponse = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", PROMPT_RESPONSE_BEAM);
                            continue;
                        }
                    };
                    this.prompt_requests.send_response(result);
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
            CREATE_TASK_BEAM => {
                for photon in photons {
                    let result: InnerTaskPayload = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", CREATE_TASK_BEAM);
                            continue;
                        }
                    };
                    this.create_task_tx.send(result).ok();
                }
            }
            UPDATE_TASK_BEAM => {
                for photon in photons {
                    let result: InnerUpdateTaskPayload = match serde_json::from_slice(&photon.payload) {
                        Ok(ok) => ok,
                        Err(_) => {
                            tracing::error!("Received invalid Photon on {}", CREATE_TASK_BEAM);
                            continue;
                        }
                    };
                    this.update_task_tx.send(result).ok();
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
    let create_task_tx: broadcast::Sender<InnerTaskPayload> = router.announce();
    let update_task_tx: broadcast::Sender<InnerUpdateTaskPayload> = router.announce();
    let job_result_tx: broadcast::Sender<JobResult> = router.announce();
    let user_created_tx: broadcast::Sender<UserCreated> = router.announce();

    // Prompts
    let mut prompt_request_rx: mpsc::Receiver<(PromptRequest, oneshot::Sender<PromptResponse>)> = router.create_channel();
    let prompt_requests = PromptResponseCollection::new();
    let handler_requests = prompt_requests.clone();

    let uri = addr.parse::<Uri>().unwrap();

    spawn(async move {
        let (voice_request_beam, voice_response_beam) = gen_voice_beams().await;
        let voice_beam = voice_response_beam.clone();

        let handle_tasks = move |wavelet: Wavelet| {
            WaveletHandler {
                job_result_tx: job_result_tx.clone(),
                job_created_tx: job_tx.clone(),
                user_created_tx: user_created_tx.clone(),
                create_task_tx: create_task_tx.clone(),
                update_task_tx: update_task_tx.clone(),
                prompt_requests: handler_requests.clone(),
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

        setup_user_beams(&mut client).await;
        setup_job_beams(&mut client).await;
        setup_task_beams(&mut client).await;
        setup_prompt_beams(&mut client).await;
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
                msg = prompt_request_rx.recv() => {
                    if let Some((msg, tx)) = msg {
                        prompt_requests.insert(msg.request_id, tx);
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(PROMPT_REQUEST_BEAM, vec).await.is_err() {
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
