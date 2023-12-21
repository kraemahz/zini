use http::Uri;
use tokio::spawn;
use tokio::sync::broadcast;

use crate::router::Router;
use crate::tables::{Project, User, Task, Flow, Graph};


pub fn prism_url(host: &str, port: &str) -> String {
    format!("ws://{}:{}", host, port)
}


pub fn emit_events(addr: &str, mut router: Router) {
    let mut user_rx: broadcast::Receiver<User> = router.subscribe();
    let mut task_rx: broadcast::Receiver<Task> = router.subscribe();
    let mut project_rx: broadcast::Receiver<Project> = router.subscribe();
    let mut flow_rx: broadcast::Receiver<Flow> = router.subscribe();
    let mut graph_rx: broadcast::Receiver<Graph> = router.subscribe();
    let uri = addr.parse::<Uri>().unwrap();

    spawn(async move {
        let mut client = match prism_client::Client::connect(uri).await {
            Ok(client) => client,
            Err(_err) => {
                tracing::warn!("Zini is running in standalone mode. No connection to prism.");
                return;
            }
        };
        tracing::info!("Zini connected to prism!");
        let user_beam = "zini::user";
        let task_beam = "zini::task";
        let project_beam = "zini::project";
        let flow_beam = "zini::flow";
        let graph_beam = "zini::graph";

        client.add_beam(user_beam).await.expect("Failed setting up client");
        client.add_beam(task_beam).await.expect("Failed setting up client");
        client.add_beam(project_beam).await.expect("Failed setting up client");
        client.add_beam(flow_beam).await.expect("Failed setting up client");
        client.add_beam(graph_beam).await.expect("Failed setting up client");

        loop {
            tokio::select!(
                msg = user_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(user_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = task_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(task_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = project_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(project_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = flow_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(flow_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
                msg = graph_rx.recv() => {
                    if let Ok(msg) = msg {
                        let vec = serde_json::to_vec(&msg).unwrap();
                        if client.emit(graph_beam, vec).await.is_err() {
                            break;
                        }
                    }
                }
            );
        }
        tracing::warn!("Prism client closed");
    });
}
