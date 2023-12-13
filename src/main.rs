use std::sync::Arc;
use tokio::sync::broadcast;
use warp::Filter;

use zini::router::Router;
use zini::session::{InvalidSessionToken, NoSessionToken, SessionStore};
use zini::tables::{establish_connection_pool, Project, User, Task};
use zini::api::*;

async fn handle_rejection(err: warp::reject::Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    if let Some(_) = err.find::<ConflictError>() {
        let json = warp::reply::json(&"Conflict: Resource already exists");
        let response = warp::reply::with_status(json, warp::http::StatusCode::CONFLICT);
        return Ok(response);
    }
    if let Some(_) = err.find::<ParseError>() {
        let json = warp::reply::json(&"Invalid parameter, parsing failed");
        let response = warp::reply::with_status(json, warp::http::StatusCode::BAD_REQUEST);
        return Ok(response);
    }
    if let Some(_) = err.find::<NotFoundError>() {
        let json = warp::reply::json(&"Not Found: Resource does not exist");
        let response = warp::reply::with_status(json, warp::http::StatusCode::NOT_FOUND);
        return Ok(response);
    }
    if let Some(_) = err.find::<InvalidSessionToken>() {
        let json = warp::reply::json(&"Unauthorized");
        let response = warp::reply::with_status(json, warp::http::StatusCode::UNAUTHORIZED);
        return Ok(response);
    }
    if let Some(_) = err.find::<NoSessionToken>() {
        let json = warp::reply::json(&"Unauthorized");
        let response = warp::reply::with_status(json, warp::http::StatusCode::UNAUTHORIZED);
        return Ok(response);
    }
    let json = warp::reply::json(&"Unhandled error");
    Ok(warp::reply::with_status(json, warp::http::StatusCode::INTERNAL_SERVER_ERROR))
}

#[tokio::main]
async fn main() {
    let password = "development";
    let database_url = format!("postgres://postgres:{}@localhost/zini", password);

    let pool = establish_connection_pool(&database_url);
    let pool = Arc::new(pool);

    let store = SessionStore::new();
    let store = Arc::new(store);

    let mut router = Router::new();
    let user_tx: broadcast::Sender<User> = router.announce();
    let task_tx: broadcast::Sender<Task> = router.announce();
    let project_tx: broadcast::Sender<Project> = router.announce();

    let routes = user::user_routes(store.clone(), pool.clone(), user_tx)
        .or(project::project_routes(store.clone(), pool.clone(), project_tx))
        .or(tasks::task_routes(store.clone(), pool.clone(), task_tx))
        .recover(handle_rejection);

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
