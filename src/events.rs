use http::Uri;
use prism_client::{AsyncClient, Wavelet};
use tokio::spawn;
use tokio::sync::broadcast;

use crate::router::Router;
use crate::tables::{Project, User, Task, Flow, Graph};


pub fn prism_url(host: &str, port: &str) -> String {
    format!("ws://{}:{}", host, port)
}

const USER_CREATED_BEAM: &str = "urn:subseq.io:oidc:user:created";
const USER_UPDATED_BEAM: &str = "urn:subseq.io:oidc:user:updated";
const JOB_RESULT_BEAM: &str = "urn:subseq.io:builds:job:result";

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


pub fn emit_events(addr: &str, mut router: Router) {
    let mut user_rx: broadcast::Receiver<User> = router.subscribe();
    let mut task_rx: broadcast::Receiver<Task> = router.subscribe();
    let mut project_rx: broadcast::Receiver<Project> = router.subscribe();
    let mut flow_rx: broadcast::Receiver<Flow> = router.subscribe();
    let mut graph_rx: broadcast::Receiver<Graph> = router.subscribe();
    let uri = addr.parse::<Uri>().unwrap();

    spawn(async move {

        async fn handle_tasks(_wavelet: Wavelet) {
        }

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
