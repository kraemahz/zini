use std::fs::File;
use std::sync::Arc;
use std::env;

use subseq_util::{
    Router,
    BaseConfig,
    InnerConfig,
    tracing::setup_tracing,
    tables::{establish_connection_pool, User},
    api::{users, sessions, handle_rejection, init_session_store},
    oidc::{init_client_pool, IdentityProvider, OidcCredentials}
};
use tokio::sync::broadcast;
use warp::Filter;

use zini::api::tasks::TaskStatePayload;
use zini::tables::{Project, Task, Flow, Graph};
use zini::api::*;


#[tokio::main]
async fn main() {
    setup_tracing("zini");

    let args: Vec<String> = env::args().collect();
    let conf_path = args.last().expect("Need a configuration file").clone();
    let conf_file = File::open(&conf_path).expect("Could not open config file");
    let conf: BaseConfig = serde_json::from_reader(conf_file).expect("Reading config failed");
    let conf: InnerConfig = conf.try_into().expect("Could not fetch all secrets from environment");

    // Database and events
    let database_url = conf.database.db_url("zini");
    let prism_url = zini::events::prism_url(&conf.prism.host, conf.prism.port);
    let pool = establish_connection_pool(&database_url).await;
    let pool = Arc::new(pool);

    // OIDC
    init_client_pool(&conf.tls.ca_path);
    let redirect_url = "https://localhost:8445/auth";
    let oidc = OidcCredentials::new(&conf.oidc.client_id,
                                    &conf.oidc.client_secret.expect("No OIDC Client Secret"),
                                    redirect_url)
        .expect("Invalid OIDC Credentials");
    let idp = IdentityProvider::new(&oidc, &conf.oidc.idp_url.to_string()).await
        .expect("Failed to establish Identity Provider connection");
    let idp = Arc::new(idp);

    // Server setup
    let session = init_session_store();

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

    let routes = projects::routes(idp.clone(), session.clone(), pool.clone(), project_tx)
        .or(users::routes(pool.clone(), user_tx))
        .or(tasks::routes(idp.clone(), session.clone(), pool.clone(), task_tx, task_update_tx))
        .or(flows::routes(idp.clone(), session.clone(), pool.clone(), flow_tx, graph_tx))
        .or(sessions::routes(session.clone(), idp.clone()))
        .recover(handle_rejection)
        .with(log_requests);

    zini::events::emit_events(&prism_url, router, pool.clone());
    warp::serve(routes)
        .tls()
        .cert_path(&conf.tls.cert_path)
        .key_path(&conf.tls.key_path)
        .run(([127, 0, 0, 1], 8445)).await;
}
