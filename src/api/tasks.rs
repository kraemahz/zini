use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use warp::{Reply, Rejection};

use crate::tables::{Project, User, Task};
use crate::session::AuthenticatedUser;
use super::*;

#[derive(Deserialize)]
pub struct TaskPayload {
    title: String, 
    description: Option<String>,
}

pub async fn create_task_handler(project: String,
                                 payload: TaskPayload,
                                 auth: AuthenticatedUser,
                                 db_pool: Arc<DbPool>,
                                 mut sender: broadcast::Sender<Task>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    let mut project = match Project::get(&mut conn, &project) {
        Some(project) => project,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let user = match User::get(&mut conn, &auth.username) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let TaskPayload{title, description} = payload;

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

pub async fn get_task_handler(task_id: String,
                              _auth: AuthenticatedUser,
                              db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let task = match Task::get(&mut conn, &task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    Ok(warp::reply::json(&task))
}

#[derive(Deserialize)]
pub struct QueryPayload {
}

#[derive(Serialize)]
pub struct QueryReply {
}

pub async fn filter_tasks_handler(payload: QueryPayload,
                                  _auth: AuthenticatedUser,
                                  db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let reply = QueryReply{};
    Ok(warp::reply::json(&reply))
}
