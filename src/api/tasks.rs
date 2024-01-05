use std::collections::HashMap;
use std::sync::Arc;
use diesel::{PgConnection, QueryResult};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};

use super::*;
use crate::tables::{
    DbPool,
    FlowConnection,
    FlowNode,
    Project,
    Task,
    TaskFlow,
    TaskLink,
    TaskUpdate,
    User,
};

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

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatePayload {
    task: Task,
    tags: Vec<String>,
    watchers: Vec<User>,
    state: FlowNode,
    links_out: Vec<TaskLink>,
    links_in: Vec<TaskLink>,
    valid_transitions: Vec<FlowNode>
}

impl TaskStatePayload {
    pub fn build(conn: &mut PgConnection, task: Task) -> QueryResult<Self> {
        let flows = task.flows(conn)?;
        let tags = task.tags(conn).ok().unwrap_or_else(Vec::new);
        let watchers = task.watchers(conn).ok().unwrap_or_else(Vec::new);
        let state = TaskFlow::get_active_node(conn, &flows)?;
        let valid_transitions = FlowConnection::edges(conn, state.id)?;
        let links_out = TaskLink::get_outgoing(conn, &task)?;
        let links_in = TaskLink::get_incoming(conn, &task)?;
        Ok(Self { task, tags, watchers, state, links_out, links_in, valid_transitions })
    }
}

async fn update_task_handler(task_id: String,
                             payload: TaskUpdate,
                             auth: AuthenticatedUser,
                             db_pool: Arc<DbPool>,
                             sender: broadcast::Sender<TaskStatePayload>) -> Result<impl Reply, Rejection> {
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
    let mut task = match Task::get(&mut conn, task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    match task.update(&mut conn, auth, payload) {
        Ok(state) => state,
        Err(_) => return Err(warp::reject::custom(DatabaseError{}))
    };

    let task_state = match TaskStatePayload::build(&mut conn, task) {
        Ok(state) => state,
        Err(_) => return Err(warp::reject::custom(DatabaseError{}))
    };
    sender.send(task_state.clone()).ok();
    Ok(warp::reply::json(&task_state))
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

    let task_payload = match TaskStatePayload::build(&mut conn, task) {
        Ok(state) => state,
        Err(_) => return Err(warp::reject::custom(DatabaseError{}))
    };
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
              task_tx: broadcast::Sender<Task>,
              task_update_tx: broadcast::Sender<TaskStatePayload>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_task = warp::post()
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_tx))
        .and_then(create_task_handler);

    let update_task = warp::put()
        .and(warp::path::param())
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_update_tx))
        .and_then(update_task_handler);

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
             .or(update_task)
             .or(get_task))
}
