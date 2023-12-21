use std::sync::Arc;
use std::env;

use tokio::sync::broadcast;
use tracing_subscriber::{prelude::*, EnvFilter};
use warp::Filter;

use zini::api::tasks::TaskStatePayload;
use zini::router::Router;
use zini::tables::{db_url, establish_connection_pool, Project, User, Task, Flow, Graph};
use zini::api::*;


fn setup_tracing() {
    let tracing_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_level(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true);

    #[cfg(debug_assertions)]
    {
        let console_layer = console_subscriber::spawn();
        let filter_layer = EnvFilter::new("zini=debug");
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(console_layer)
            .with(tracing_layer)
            .init();
    }
    #[cfg(not(debug_assertions))]
    {
        let filter_layer = EnvFilter::new("zini=info");
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(tracing_layer)
            .init();
    }

    tracing::info!("Zini started");
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let default_password = "development".to_string();

    let pg_username = env::var("PG_USERNAME").unwrap_or("postgres".to_string());
    let pg_password = env::var("PG_PASSWORD").unwrap_or(default_password.clone());
    let pg_host = env::var("PG_HOST").unwrap_or("localhost".to_string());

    let prism_host = env::var("PRISM_HOST").unwrap_or("localhost".to_string());
    let prism_port = env::var("PRISM_PORT").unwrap_or("5050".to_string());

    if default_password == pg_password {
        tracing::warn!("Zini is running in development mode with the default password.");
    }
    let database_url = db_url(&pg_username, &pg_host, &pg_password, "zini");
    let prism_url = zini::events::prism_url(&prism_host, &prism_port);

    let pool = establish_connection_pool(&database_url).await;
    let pool = Arc::new(pool);

    let store = SessionStore::new();
    let store = Arc::new(store);

    let mut router = Router::new();
    let user_tx: broadcast::Sender<User> = router.announce();
    let task_tx: broadcast::Sender<Task> = router.announce();
    let project_tx: broadcast::Sender<Project> = router.announce();
    let flow_tx: broadcast::Sender<Flow> = router.announce();
    let graph_tx: broadcast::Sender<Graph> = router.announce();
    let task_update_tx: broadcast::Sender<TaskStatePayload> = router.announce();

    let log_requests = warp::log::custom(|info| {
        tracing::info!("{} {} {} {}",
                       info.remote_addr()
                           .map(|addr| addr.to_string())
                           .unwrap_or_else(|| "???".into()),
                       info.method(),
                       info.path(),
                       info.status());
    });

    let routes = users::routes(store.clone(), pool.clone(), user_tx)
        .or(projects::routes(store.clone(), pool.clone(), project_tx))
        .or(tasks::routes(store.clone(), pool.clone(), task_tx, task_update_tx))
        .or(flows::routes(store.clone(), pool.clone(), flow_tx, graph_tx))
        .or(sessions::routes(store.clone(), pool.clone()))
        .recover(handle_rejection)
        .with(log_requests);

    zini::events::emit_events(&prism_url, router);
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
