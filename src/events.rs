use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Future;
use http::Uri;
use prism_client::{AsyncClient, Wavelet};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use subseq_util::router::Router;
use subseq_util::tables::{DbPool, UserTable};
use tokio::spawn;
use tokio::time::sleep;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::api::prompts::{InitializePromptChannel, PromptRx, PromptTx};
use crate::api::tasks::TaskRun;
use crate::api::voice::{
    SpeechToText, SpeechToTextRequest, SpeechToTextResponse, VoiceResponseCollection,
};
use crate::interop::{JobRequest, JobResponse};
use crate::{
    api::prompts::PromptResponseCollection,
    api::jobs::JobRequestCollection,
    api::tasks::TaskStatePayload,
    interop::{DenormalizedJob, JobResult},
    tables::{Flow, Graph, Project, Task, User},
};

pub fn prism_url(host: &str, port: Option<u16>) -> String {
    let port_str = if let Some(port) = port {
        format!(":{}", port)
    } else {
        String::new()
    };
    format!("ws://{}{}", host, port_str)
}

// Users
pub const USER_CREATED_BEAM: &str = "urn:subseq.io:oidc:user:created";
const USER_UPDATED_BEAM: &str = "urn:subseq.io:oidc:user:updated";

// Builds from Sage
const JOB_CREATED_BEAM: &str = "urn:subseq.io:builds:job:created";
const JOB_PROGRESS_BEAM: &str = "urn:subseq.io:builds:job:progress";
const JOB_REQUEST_BEAM: &str = "urn:subseq.io:builds:job:request";
// Tasks from Sage
const TASK_RUN_BEAM: &str = "urn:subseq.io:tasks:task:run";
const TASK_RESULT_BEAM: &str = "urn:subseq.io:tasks:task:result";

// Task Broadcast
const TASK_CREATED_BEAM: &str = "urn:subseq.io:tasks:task:created";
const TASK_UPDATED_BEAM: &str = "urn:subseq.io:tasks:task:updated";
const TASK_ASSIGNEE_BEAM: &str = "urn:subseq.io:tasks:task:assignee:changed";
const TASK_STATE_BEAM: &str = "urn:subseq.io:tasks:task:state:changed";

// Projects
pub const PROJECT_CREATED_BEAM: &str = "urn:subseq.io:tasts:project:created";
const PROJECT_UPDATED_BEAM: &str = "urn:subseq.io:tasks:project:updated";

// Flows
const FLOW_CREATED_BEAM: &str = "urn:subseq.io:tasks:workflow:created";
const FLOW_UPDATED_BEAM: &str = "urn:subseq.io:tasks:workflow:updated";

const DEFAULT_PING_RATE: Duration = Duration::from_secs(50);

async fn setup_user_beams(client: &mut AsyncClient) {
    client
        .add_beam(USER_CREATED_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(USER_UPDATED_BEAM)
        .await
        .expect("Failed setting up client");

    client
        .subscribe(USER_CREATED_BEAM, None)
        .await
        .expect("Failed subscribing");
    client
        .subscribe(USER_UPDATED_BEAM, None)
        .await
        .expect("Failed subscribing");
}

async fn setup_job_beams(client: &mut AsyncClient) {
    client
        .subscribe(TASK_RESULT_BEAM, None)
        .await
        .expect("Failed subscribing");
    client
        .subscribe(JOB_CREATED_BEAM, None)
        .await
        .expect("Failed subscribing");
    client
        .subscribe(JOB_REQUEST_BEAM, None)
        .await
        .expect("Failed subscribing");
    client
        .subscribe(JOB_PROGRESS_BEAM, None)
        .await
        .expect("Failed subscribing");
}

async fn setup_task_beams(client: &mut AsyncClient) {
    client
        .add_beam(TASK_CREATED_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(TASK_UPDATED_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(TASK_ASSIGNEE_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(TASK_STATE_BEAM)
        .await
        .expect("Failed setting up client");
}

async fn setup_project_beams(client: &mut AsyncClient) {
    client
        .add_beam(PROJECT_CREATED_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(PROJECT_UPDATED_BEAM)
        .await
        .expect("Failed setting up client");
}

async fn setup_flow_beams(client: &mut AsyncClient) {
    client
        .add_beam(FLOW_CREATED_BEAM)
        .await
        .expect("Failed setting up client");
    client
        .add_beam(FLOW_UPDATED_BEAM)
        .await
        .expect("Failed setting up client");
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

impl From<User> for UserCreated {
    fn from(user: User) -> Self {
        Self(user)
    }
}

pub fn create_users_from_events(
    mut user_created_rx: broadcast::Receiver<UserCreated>,
    db_pool: Arc<DbPool>,
) {
    spawn(async move {
        while let Ok(created_user) = user_created_rx.recv().await {
            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => return,
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
    beam: String,
}

struct WaveletHandler {
    job_result_tx: mpsc::Sender<JobResult>,
    job_created_tx: mpsc::Sender<DenormalizedJob>,
    user_created_tx: broadcast::Sender<UserCreated>,
    job_requests: JobRequestCollection,
    prompt_requests: PromptResponseCollection,
    prompt_beam: String,
    voice_requests: VoiceResponseCollection,
    voice_beam: String,
    wavelet: Wavelet,
}

macro_rules! process_photons {
    ($beam:expr, $photons:expr, $result_type:ty, $tx:expr) => {
        for photon in $photons {
            let result: $result_type = match serde_json::from_slice(&photon.payload) {
                Ok(ok) => ok,
                Err(err) => {
                    tracing::error!("Received invalid Photon on {}: {:?}", $beam, err);
                    continue;
                }
            };
            $tx.send(result.into()).ok();
        }
    };
}

macro_rules! process_photons_spawn {
    ($beam:expr, $photons:expr, $result_type:ty, $tx:expr) => {
        tracing::info!("Received {}", $beam);
        let payloads: Vec<_> = $photons.into_iter().map(|ph| ph.payload.clone()).collect();
        let tx = $tx.clone();
        spawn(async move {
            for payload in payloads {
                let result: $result_type = match serde_json::from_slice(&payload) {
                    Ok(ok) => ok,
                    Err(err) => {
                        tracing::error!("Received invalid Photon on {}: {:?}", $beam, err);
                        continue;
                    }
                };
                tx.send(result.into()).await.ok();
            }
        });
    };
}

macro_rules! emit_photon {
    ($client:expr, $msg:expr, $beam:expr) => {
        if let Ok(msg) = $msg {
            tracing::info!("Emit {}", $beam);
            let vec = serde_json::to_vec(&msg).unwrap();
            if $client.emit($beam, vec).await.is_err() {
                break;
            }
        }
    };
}

macro_rules! process_dynamic_photons {
    ($beam:expr, $photons:expr, $result_type:ty, $tx:expr, $deser_fn:path) => {
        for photon in $photons {
            let result: $result_type = match $deser_fn(&photon.payload) {
                Ok(ok) => ok,
                Err(err) => {
                    tracing::error!("Received invalid Photon on {}: {:?}", $beam, err);
                    continue;
                }
            };
            $tx.send_response(result);
        }
    };
}

impl Future for WaveletHandler {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Wavelet { beam, photons } = &this.wavelet;
        match beam.as_str() {
            JOB_REQUEST_BEAM => {
                tracing::info!("Received {}", JOB_REQUEST_BEAM);
                let payloads: Vec<_> = photons.into_iter().map(|ph| ph.payload.clone()).collect();
                let requests = this.job_requests.clone();
                spawn(async move {
                    for payload in payloads {
                        let result: JobRequest = match serde_json::from_slice(&payload) {
                            Ok(ok) => ok,
                            Err(err) => {
                                tracing::error!("Received invalid Photon on {}: {:?}", JOB_REQUEST_BEAM, err);
                                continue;
                            }
                        };
                        requests.insert(result).await;
                    }
                });

            }
            TASK_RESULT_BEAM => {
                process_photons_spawn!(TASK_RESULT_BEAM, photons, JobResult, this.job_result_tx);
            }
            JOB_CREATED_BEAM => {
                process_photons_spawn!(JOB_CREATED_BEAM, photons, DenormalizedJob, this.job_created_tx);
            }
            USER_CREATED_BEAM => {
                process_photons!(USER_CREATED_BEAM, photons, User, this.user_created_tx);
            }
            b => {
                if b == this.voice_beam {
                    process_dynamic_photons!(b,
                                             photons,
                                             SpeechToTextResponse,
                                             this.voice_requests,
                                             serde_cbor::from_slice);
                } else if b == this.prompt_beam {
                    process_dynamic_photons!(b,
                                             photons,
                                             PromptRx,
                                             this.prompt_requests,
                                             serde_json::from_slice);
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
    let mut task_run_rx: broadcast::Receiver<TaskRun> = router.subscribe();
    let mut project_rx: broadcast::Receiver<Project> = router.subscribe();
    let mut flow_rx: broadcast::Receiver<Flow> = router.subscribe();
    let mut graph_rx: broadcast::Receiver<Graph> = router.subscribe();

    // Voice
    let mut voice_rx: mpsc::Receiver<(SpeechToText, oneshot::Sender<SpeechToTextResponse>)> =
        router.create_channel();
    let voice_requests = VoiceResponseCollection::new();
    let handler_voice_requests = voice_requests.clone();

    // Jobs
    let job_tx: mpsc::Sender<DenormalizedJob> = router.get_address().cloned().expect("job_tx");
    let job_result_tx: mpsc::Sender<JobResult> = router.get_address().cloned().expect("job_result_tx");
    let job_request_tx: mpsc::Sender<JobRequest> = router.get_address().cloned().expect("job_request_tx");
    let job_requests = JobRequestCollection::new(job_request_tx);

    let mut job_response_rx: broadcast::Receiver<JobResponse> = router.subscribe();
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
        let job_requests_wave = job_requests.clone();

        let handle_tasks = move |wavelet: Wavelet| WaveletHandler {
            job_result_tx: job_result_tx.clone(),
            job_requests: job_requests_wave.clone(),
            job_created_tx: job_tx.clone(),
            user_created_tx: user_created_tx.clone(),
            prompt_requests: handler_requests.clone(),
            prompt_beam: prompt_beam.clone(),
            voice_requests: handler_voice_requests.clone(),
            voice_beam: voice_beam.clone(),
            wavelet,
        };

        let mut client = match AsyncClient::connect(uri, handle_tasks).await {
            Ok(client) => client,
            Err(_err) => {
                tracing::warn!("Zini is running in standalone mode. No connection to prism.");
                return;
            }
        };
        tracing::info!("Zini connected to prism!");

        client
            .add_beam(&voice_request_beam)
            .await
            .expect("Failed setting up client");
        client
            .subscribe(&voice_response_beam, None)
            .await
            .expect("Failed subscribing");
        client
            .add_beam(&prompt_request_beam)
            .await
            .expect("Failed setting up client");
        client
            .subscribe(&prompt_response_beam, None)
            .await
            .expect("Failed subscribing");

        setup_user_beams(&mut client).await;
        setup_job_beams(&mut client).await;
        setup_task_beams(&mut client).await;
        setup_project_beams(&mut client).await;
        setup_flow_beams(&mut client).await;

        let mut remaining_time = DEFAULT_PING_RATE;
        let mut instant = Instant::now();
        loop {
            remaining_time = remaining_time.checked_sub(instant.elapsed()).unwrap_or(Duration::ZERO);
            let ping_timer = sleep(remaining_time);
            tokio::select!(
                _ = ping_timer => {
                    if client.ping().await.is_err() {
                        break;
                    }
                    remaining_time = DEFAULT_PING_RATE;
                    instant = Instant::now();
                }
                msg = job_response_rx.recv() => {
                    if let Ok(msg) = msg {
                        let JobResponse{job_id, response} = msg;
                        let beam = job_requests.remove(job_id);
                        if let Some(beam) = beam {
                            tracing::info!("Emit {}", beam);
                            let vec = serde_json::to_vec(&response).unwrap();
                            if client.emit(beam, vec).await.is_err() {
                                break;
                            }
                        }
                    }
                }
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
                    if let Some(InitializePromptChannel(stream_id, mut rx, tx)) = msg {
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
                    emit_photon!(client, msg, USER_CREATED_BEAM);
                }
                msg = task_rx.recv() => {
                    emit_photon!(client, msg, TASK_CREATED_BEAM);
                }
                msg = task_update_rx.recv() => {
                    emit_photon!(client, msg, TASK_UPDATED_BEAM);
                }
                msg = task_run_rx.recv() => {
                    if let Ok(msg) = msg {
                        let msg = msg.state;
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(TASK_RUN_BEAM, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = project_rx.recv() => {
                    emit_photon!(client, msg, PROJECT_CREATED_BEAM);
                }
                msg = flow_rx.recv() => {
                    emit_photon!(client, msg, FLOW_CREATED_BEAM);
                }
                msg = graph_rx.recv() => {
                    emit_photon!(client, msg, FLOW_UPDATED_BEAM);
                }
            );
        }
        tracing::warn!("Prism client closed");
    });
}
