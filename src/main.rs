use std::sync::Arc;
use diesel::r2d2::{ConnectionManager, Pool};
use tokio::sync::broadcast;
use warp::Filter;
use diesel::prelude::*;

use zini::router::{Router, with_broadcast};
use zini::session::{InvalidSessionToken, NoSessionToken, SessionStore, authenticate};
use zini::tables::{Project, User, Task};
use zini::api::*;

fn establish_connection_pool(database_url: &str) -> DbPool {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .build(manager)
        .expect("Failed to create pool.")
}

fn with_db(pool: Arc<DbPool>) -> impl Filter<Extract = (Arc<DbPool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}

async fn handle_rejection(err: warp::reject::Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    if let Some(_) = err.find::<ConflictError>() {
        let json = warp::reply::json(&"Conflict: Resource already exists");
        let response = warp::reply::with_status(json, warp::http::StatusCode::CONFLICT);
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

    let create_user_route = warp::post()
        .and(warp::path("user"))
        .and(warp::body::json())
        .and(with_db(pool.clone()))
        .and(with_broadcast(user_tx))
        .and_then(zini::api::user::create_user_handler);

    let create_project_route = warp::post()
        .and(warp::path("project"))
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(project_tx))
        .and_then(zini::api::project::create_project_handler);

    let get_project_route = warp::get()
        .and(warp::path("project"))
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(zini::api::project::get_project_handler);

    let create_task_route = warp::post()
        .and(warp::path("project"))
        .and(warp::path::param())
        .and(warp::path("task"))
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_tx))
        .and_then(zini::api::tasks::create_task_handler);

    let get_task_route = warp::get()
        .and(warp::path("task"))
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(zini::api::tasks::get_task_handler);

    let filter_tasks_route = warp::post()
        .and(warp::path("task"))
        .and(warp::path("query"))
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(zini::api::tasks::filter_tasks_handler);

    let routes = create_user_route
        .or(create_project_route)
        .or(get_project_route)
        .or(create_task_route)
        .or(get_task_route)
        .or(filter_tasks_route)
        .recover(handle_rejection);

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}

