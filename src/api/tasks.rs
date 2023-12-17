use std::collections::HashMap;
use std::sync::Arc;
use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};

use super::*;
use crate::router::with_broadcast;
use crate::tables::{DbPool, Project, User, Task, FlowNode, FlowConnection, TaskFlow};

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

    let user = match User::get(&mut conn, auth.id()) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };

    let task = match Task::create(
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

    let mut dict = HashMap::new();
    dict.insert("task", task.id.to_string());
    Ok(warp::reply::json(&dict))
}

#[derive(Serialize)]
pub struct TaskOutPayload {
    task: Task,
    tags: Vec<String>,
    watchers: Vec<User>,
    state: FlowNode,
    valid_transitions: Vec<FlowNode>
}

fn get_active_node(conn: &mut PgConnection, flows: &[TaskFlow]) -> Result<FlowNode, Rejection> {
    let flow = match flows.first() {
        Some(flow) => flow,
        None => return Err(warp::reject::custom(DatabaseError{}))
    };

    let node_id = match flow.current_node_id {
        Some(node_id) => node_id,
        None => return Err(warp::reject::custom(DatabaseError{}))
    };

    match FlowNode::get(conn, node_id) {
        Some(node) => Ok(node),
        None => Err(warp::reject::custom(DatabaseError{}))
    }
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

    let flows = match task.flows(&mut conn) {
        Ok(flows) => flows,
        Err(_) => return Err(warp::reject::custom(DatabaseError{}))
    };
    let tags = task.tags(&mut conn).ok().unwrap_or_else(Vec::new);
    let watchers = task.watchers(&mut conn).ok().unwrap_or_else(Vec::new);
    let state = get_active_node(&mut conn, &flows)?;
    let valid_transitions = match FlowConnection::edges(&mut conn, state.id) {
        Ok(valid) => valid,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let task_payload = TaskOutPayload { task, tags, watchers, state, valid_transitions };
    Ok(warp::reply::json(&task_payload))
}

#[derive(Deserialize)]
pub struct QueryPayload {
    page: u32,
    page_size: u32,
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
    let tasks = Task::query(&mut conn, &payload.query, payload.page, payload.page_size);
    let reply = QueryReply{tasks};
    Ok(warp::reply::json(&reply))
}


pub fn routes(store: Arc<SessionStore>,
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

    let filter_tasks = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(filter_tasks_handler);

    warp::path("task")
        .and(filter_tasks
             .or(create_task)
             .or(get_task))
}
