use std::pin::Pin;
use std::task::{Context, Poll};

use http::Uri;
use futures::Future;
use prism_client::{AsyncClient, Wavelet};
use tokio::spawn;
use tokio::sync::broadcast;
use subseq_util::router::Router;

use crate::{
    interop::{Job, JobResult},
    tables::{Project, Task, Flow, Graph, User},
    api::tasks::TaskStatePayload
};


pub fn prism_url(host: &str, port: &str) -> String {
    format!("ws://{}:{}", host, port)
}

const USER_CREATED_BEAM: &str = "urn:subseq.io:oidc:user:created";
const USER_UPDATED_BEAM: &str = "urn:subseq.io:oidc:user:updated";

const JOB_RESULT_BEAM: &str = "urn:subseq.io:builds:job:result";
const JOB_CREATED_BEAM: &str = "urn:subseq.io:builds:k8s:job:created";

const TASK_CREATED_BEAM: &str = "urn:subseq.io:tasks::task:created";
const TASK_UPDATED_BEAM: &str = "urn:subseq.io:tasks::task:updated";
const TASK_ASSIGNEE_BEAM: &str = "urn:subseq.io:tasks::task:assignee::changed";
const TASK_STATE_BEAM: &str = "urn:subseq.io:tasks::task:state:changed";

const PROJECT_CREATED_BEAM: &str = "urn:subseq.io:tasts:project:created";
const PROJECT_UPDATED_BEAM: &str = "urn:subseq.io:tasks:project:updated";

const FLOW_CREATED_BEAM: &str = "urn:subseq.io:tasks:workflow:created";
const FLOW_UPDATED_BEAM: &str = "urn:subseq.io:tasks:workflow:updated";


async fn setup_user_beams(client: &mut AsyncClient) {
    client.add_beam(USER_CREATED_BEAM).await.expect("Failed setting up client");
    client.add_beam(USER_UPDATED_BEAM).await.expect("Failed setting up client");
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

struct WaveletHandler {
    job_result_tx: broadcast::Sender<JobResult>,
    job_created_tx: broadcast::Sender<Job>,
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
            b => {
                tracing::error!("Received unhandled Beam: {}", b);
            }
        }

        Poll::Ready(())
    }
}


pub fn emit_events(addr: &str, mut router: Router) {
    let mut user_rx: broadcast::Receiver<User> = router.subscribe();
    let mut task_rx: broadcast::Receiver<Task> = router.subscribe();
    let mut task_update_rx: broadcast::Receiver<TaskStatePayload> = router.subscribe();

    let mut project_rx: broadcast::Receiver<Project> = router.subscribe();
    let mut flow_rx: broadcast::Receiver<Flow> = router.subscribe();
    let mut graph_rx: broadcast::Receiver<Graph> = router.subscribe();

    let job_tx: broadcast::Sender<Job> = router.announce();
    let job_result_tx: broadcast::Sender<JobResult> = router.announce();

    let uri = addr.parse::<Uri>().unwrap();

    spawn(async move {
        let handle_tasks = move |wavelet: Wavelet| {
            WaveletHandler {
                job_result_tx: job_result_tx.clone(),
                job_created_tx: job_tx.clone(),
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
        setup_user_beams(&mut client).await;
        setup_task_beams(&mut client).await;
        setup_project_beams(&mut client).await;
        setup_flow_beams(&mut client).await;

        loop {
            tokio::select!(
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
