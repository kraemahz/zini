use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};

use super::*;
use crate::router::with_broadcast;
use crate::tables::{DbPool, Project, User, Task};
use crate::session::{AuthenticatedUser, SessionStore, authenticate};

#[derive(Deserialize)]
pub struct TaskPayload {
    project_id: Uuid,
    title: String, 
    description: Option<String>,
}

async fn create_task_handler(payload: TaskPayload,
                             auth: AuthenticatedUser,
                             db_pool: Arc<DbPool>,
                             mut sender: broadcast::Sender<Task>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    let TaskPayload{project_id, title, description} = payload;

    let mut project = match Project::get(&mut conn, project_id) {
        Some(project) => project,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let user = match User::get(&mut conn, auth.0) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    match Task::create(
        &mut conn,
        &mut sender,
        &mut project,
        &title,
        &description.unwrap_or_else(String::new),
        &user,
    ) {
        Ok(task) => task,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };

    Ok(warp::reply::json(&"Task created"))
}

async fn get_task_handler(task_id: String,
                          _auth: AuthenticatedUser,
                          db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let task_id = match Uuid::parse_str(&task_id) {
        Ok(task_id) => task_id,
        Err(_) => {
            return Err(warp::reject::custom(ParseError{}));
        }
    };
    let task = match Task::get(&mut conn, task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    Ok(warp::reply::json(&task))
}

#[derive(Deserialize)]
pub struct QueryPayload {
    page: i64,
    page_size: i64,
    query: HashMap<String, String>
}

#[derive(Serialize)]
pub struct QueryReply {
    tasks: Vec<Task>
}

async fn filter_tasks_handler(payload: QueryPayload,
                              _auth: AuthenticatedUser,
                              db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let tasks = Task::query(&mut conn, &payload.query, payload.page, payload.page_size)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    let reply = QueryReply{tasks};
    Ok(warp::reply::json(&reply))
}


pub fn task_routes(store: Arc<SessionStore>,
                   pool: Arc<DbPool>,
                   task_tx: broadcast::Sender<Task>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_task = warp::post()
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_tx))
        .and_then(create_task_handler);

    let get_task = warp::get()
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_task_handler);

    let filter_tasks = warp::post()
        .and(warp::path("query"))
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(filter_tasks_handler);

    warp::path("task")
        .and(create_task
             .or(get_task)
             .or(filter_tasks))
}
