use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use subseq_util::{
    Router,
    BaseConfig,
    InnerConfig,
    tracing::setup_tracing,
    tables::establish_connection_pool,
    api::{sessions, handle_rejection, init_session_store, users as util_users},
    oidc::{init_client_pool, IdentityProvider, OidcCredentials}
};
use warp::{Filter, reject::Rejection};

use zini::api::*;
use zini::tables::User;
use zini::events;


#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    conf: PathBuf,
}

const ZINI_PORT: u16 = 8445;

#[tokio::main]
async fn main() {
    setup_tracing("zini");
    let args = Args::parse();
    let conf_file = File::open(args.conf).expect("Could not open file");
    let conf: BaseConfig = serde_json::from_reader(conf_file).expect("Reading config failed");
    let conf: InnerConfig = conf.try_into().expect("Could not fetch all secrets from environment");

    // Database and events
    let database_url = conf.database.db_url("zini");
    let prism_url = zini::events::prism_url(&conf.prism.host, conf.prism.port);
    let pool = establish_connection_pool(&database_url).await;
    let pool = Arc::new(pool);

    // OIDC
    let (idp, tls) = match conf.oidc.as_ref() {
        Some(oidc_conf) => {
            let tls = conf.tls.as_ref().expect("Must define TLS conf with OIDC conf");
            init_client_pool(tls.ca_path.as_str());
            let redirect_url = format!("https://localhost:{ZINI_PORT}/auth");
            let oidc = OidcCredentials::new(oidc_conf.client_id.as_str(),
                                            oidc_conf.client_secret.clone().expect("No OIDC Client Secret"),
                                            redirect_url)
                .expect("Invalid OIDC Credentials");
            let idp = IdentityProvider::new(&oidc, &oidc_conf.idp_url.to_string()).await
                .expect("Failed to establish Identity Provider connection");
            (Some(Arc::new(idp)), Some(tls))
        }
        None => (None, None)
    };

    // Server setup
    let session = init_session_store();

    let mut router = Router::new();
    events::emit_events(&prism_url, &mut router, pool.clone());
    tasks::create_task_worker(pool.clone(), &mut router);
    tasks::update_task_worker(pool.clone(), &mut router);

    let log_requests = warp::log::custom(|info| {
        tracing::info!("{} {} {} {}",
                       info.remote_addr()
                           .map(|addr| addr.to_string())
                           .unwrap_or_else(|| "???".into()),
                       info.method(),
                       info.path(),
                       info.status());
    });

    let probe = warp::path("probe")
        .and(warp::get())
        .and_then(|| async {Ok::<_, Rejection>(warp::reply::html("<html>up</html>"))});
    let frontend = warp::path::end().and(warp::fs::file("dist/index.html"));
    let ico = warp::path("favicon.ico").and(warp::fs::file("dist/favicon.ico"));
    let logo = warp::path("subseq-logo.svg").and(warp::fs::file("dist/subseq-logo.svg"));
    let assets = warp::path("assets").and(warp::fs::dir("dist/assets"));

    let routes = projects::routes(idp.clone(), session.clone(), pool.clone(), &mut router)
        .or(util_users::routes::<User>(idp.clone(), session.clone(), pool.clone(), &mut router))
        .or(users::routes(idp.clone(), session.clone(), pool.clone()))
        .or(tasks::routes(idp.clone(), session.clone(), pool.clone(), &mut router))
        .or(flows::routes(idp.clone(), session.clone(), pool.clone(), &mut router))
        .or(voice::routes(idp.clone(), session.clone(), &mut router))
        .or(probe)
        .or(frontend)
        .or(ico)
        .or(logo)
        .or(assets);

    if let Some(idp) = idp {
        let routes = routes.or(sessions::routes(session, idp))
            .recover(handle_rejection)
            .with(log_requests);
        let tls = tls.unwrap();
        warp::serve(routes)
            .tls()
            .cert_path(tls.cert_path.as_str())
            .key_path(tls.key_path.as_str())
            .run(([127, 0, 0, 1], ZINI_PORT)).await;
    } else {
        let routes = routes.or(sessions::no_auth_routes(session))
            .recover(handle_rejection)
            .with(log_requests);
        warp::serve(routes).run(([127, 0, 0, 1], ZINI_PORT)).await;
    }
}
