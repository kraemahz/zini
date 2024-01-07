use std::sync::Arc;
use std::env;

use subseq_util::{
    Router,
    tracing::setup_tracing,
    tables::{establish_connection_pool, PgVars, User},
    api::{users, sessions, handle_rejection}, oidc::{IdentityProvider, OidcCredentials}
};
use tokio::sync::broadcast;
use warp::Filter;
use warp_sessions::MemoryStore;

use zini::api::tasks::TaskStatePayload;
use zini::tables::{Project, Task, Flow, Graph};
use zini::api::*;


#[tokio::main]
async fn main() {
    setup_tracing("zini");
    let default_password = "development";
    let pg_vars = PgVars::new(default_password);

    let prism_host = env::var("PRISM_HOST").unwrap_or("localhost".to_string());
    let prism_port = env::var("PRISM_PORT").unwrap_or("5050".to_string());

    let oidc_provider = env::var("OIDC_IDP_URL")
        .unwrap_or("https://localhost".to_string());
    let oidc_client_id = env::var("OIDC_CLIENT_ID")
        .unwrap_or("zini-tasks".to_string());
    let oidc_client_secret = env::var("OIDC_CLIENT_SECRET")
        .unwrap_or("".to_string());
    let redirect_url = "/sessions/auth";

    let database_url = pg_vars.db_url("zini");
    let prism_url = zini::events::prism_url(&prism_host, &prism_port);

    let pool = establish_connection_pool(&database_url).await;
    let pool = Arc::new(pool);

    let oidc = OidcCredentials::new(oidc_client_id, oidc_client_secret, redirect_url)
        .expect("Invalid OIDC Credentials");
    let idp = IdentityProvider::new(&oidc, &oidc_provider).await
        .expect("Failed to establish Identity Provider connection");
    let idp = Arc::new(idp);
    let session = MemoryStore::new();

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

    let routes = projects::routes(idp.clone(), pool.clone(), project_tx)
        .or(users::routes(pool.clone(), user_tx))
        .or(tasks::routes(idp.clone(), pool.clone(), task_tx, task_update_tx))
        .or(flows::routes(idp.clone(), pool.clone(), flow_tx, graph_tx))
        .or(sessions::routes(session.clone(), idp.clone()))
        .recover(handle_rejection)
        .with(log_requests);

    zini::events::emit_events(&prism_url, router);
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
