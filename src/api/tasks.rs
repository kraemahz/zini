use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use diesel::{PgConnection, QueryResult};
use serde::{Deserialize, Serialize};
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::{api::*, Router, tables::UserTable};
use subseq_util::oidc::IdentityProvider;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::spawn;
use tokio::time::timeout;
use uuid::Uuid;
use warp::{Filter, Reply, Rejection};
use warp_sessions::{MemoryStore, SessionWithStore};

use crate::api::users::DenormalizedUser;
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
use super::with_channel;
use super::prompts::{PromptChannel, PromptResponseType, PromptRequest, PromptResponse};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskPayload {
    project_id: Uuid,
    title: Option<String>, 
    description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InnerTaskPayload {
    user_id: Uuid,
    task_id: Uuid,
    project_id: Uuid,
    title: Option<String>, 
    description: String,
    tags: Vec<String>,
    components: Vec<String>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InnerUpdateTaskPayload {
    user_id: Uuid,
    task_id: Uuid,
    update: TaskUpdate
}

#[derive(Deserialize, Default)]
pub struct TitleJson {
    title: String
}

#[derive(Deserialize, Serialize, Default)]
pub struct DescriptionJson {
    description: String
}

async fn title_from_description(
    project_id: Uuid,
    user_id: Uuid,
    description: &str,
    prompt_request_tx: &mut mpsc::Sender<PromptChannel>) -> Option<String>
{
    let (tx, rx) = oneshot::channel();
    let request = PromptRequest{
        request_id: Uuid::new_v4(),
        project_id,
        user_id,
        prompt_id: "tools_aNDkkK".into(),
        prompt: description.to_string()
    };
    if prompt_request_tx.send((request, tx)).await.is_err() {
        return None;
    }
    let response = match timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(response)) => response,
        Err(_) => {
            tracing::warn!("Timed out waiting for title");
            return None;
        }
        Ok(Err(_)) => {
            return None;
        }
    };
    let PromptResponse{request_id: _, response, error} = response;
    match response {
        Some(PromptResponseType::Json(message)) => {
            match serde_json::from_value::<TitleJson>(message) {
                Ok(title) => Some(title.title),
                Err(err) => {
                    tracing::error!("Response deserialization error: {}", err);
                    None
                }
            }
        }
        Some(PromptResponseType::Text(message)) => Some(message),
        None => {
            tracing::error!("Prompt error: {:?}", error);
            None
        }
    }
}

pub fn update_task_worker(db_pool: Arc<DbPool>, router: &mut Router) {
    let sender = router.announce::<TaskStatePayload>();
    let mut receiver = router.subscribe::<InnerUpdateTaskPayload>();

    spawn(async move {
        loop {
            let payload = receiver.recv().await;
            let payload = match payload {
                Ok(payload) => payload,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!("Missed {} messages on InnerUpdateTaskPayload!", n);
                    continue;
                }
                Err(_) => {
                    break;
                }
            };
            let InnerUpdateTaskPayload{user_id, task_id, update} = payload;

            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => {
                    tracing::error!("Database error");
                    continue;
                }
            };

            let mut task = match Task::get(&mut conn, task_id) {
                Some(task) => task,
                None => {
                    tracing::warn!("Task not found: {}", task_id);
                    continue;
                }
            };

            task.update(&mut conn, user_id, update).ok();
            let payload = match TaskStatePayload::build(&mut conn, task) {
                Ok(task) => task,
                Err(_) => {
                    tracing::warn!("Could not build TaskStatePayload");
                    continue;
                },
            };
            sender.send(payload).ok();
        }
    });
}

pub fn create_task_worker(db_pool: Arc<DbPool>, router: &mut Router) {
    let sender = router.announce::<Task>();
    let mut receiver = router.subscribe::<InnerTaskPayload>();
    let mut prompt_request_tx: mpsc::Sender<PromptChannel> = router.get_address()
        .expect("Prompt channel undefined").clone();

    spawn(async move {
        loop {
            let payload = receiver.recv().await;
            let payload = match payload {
                Ok(payload) => payload,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!("Missed {} messages on InnerTaskPayload!", n);
                    continue;
                }
                Err(_) => {
                    break;
                }
            };

            let InnerTaskPayload{user_id, task_id, project_id, title, description, tags, components} = payload;
            let title = if let Some(title) = title { title } else {
                match title_from_description(project_id, user_id, &description, &mut prompt_request_tx).await {
                    Some(title) => title,
                    None => {
                        tracing::error!("Failed to create title for issue");
                        continue;
                    }
                }
            };

            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => {
                    tracing::error!("Database error");
                    continue;
                }
            };

            let user = match User::get(&mut conn, user_id) {
                Some(user) => user,
                None => {
                    tracing::error!("Could not find user {}!", user_id);
                    continue;
                }
            };

            let mut project = match Project::get(&mut conn, project_id) {
                Some(project) => project,
                None => {
                    tracing::error!("No such project: {}", project_id);
                    continue;
                }
            };

            let task = match Task::create(
                &mut conn,
                task_id,
                &mut project,
                &title,
                &description,
                &user,
            ) {
                Ok(task) => task,
                Err(_) => {
                    tracing::error!("Failed to create task");
                    continue;
                }
            };
            sender.send(task.clone()).ok();
            for tag in tags {
                let label = format!("{{\"label\": \"{}\"}}", tag);
                task.add_tag(&mut conn, &label).ok();
            }
            for component in components {
                let component = format!("{{\"component\": \"{}\"}}", component);
                task.add_tag(&mut conn, &component).ok();
            }
        }
    });
}


async fn create_task_handler(payload: TaskPayload,
                             auth: AuthenticatedUser,
                             session: SessionWithStore<MemoryStore>,
                             db_pool: Arc<DbPool>,
                             mut prompt_request_tx: mpsc::Sender<PromptChannel>,
                             sender: broadcast::Sender<Task>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    // We explicitly ignore setting the task id from the api.
    let TaskPayload{project_id, title, description} = payload;

    let title = if let Some(title) = title { title } else {
        title_from_description(project_id, auth.id(), &description, &mut prompt_request_tx).await
            .ok_or_else(|| warp::reject::custom(InvalidConfigurationError{}))?
    };

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
        Uuid::new_v4(),
        &mut project,
        &title,
        &description,
        &user,
    ) {
        Ok(task) => task,
        Err(err) => {
            tracing::error!("Task creation failed: {:?}", err);
            return Err(warp::reject::custom(ConflictError{}));
        }
    };
    sender.send(task.clone()).ok();
    let payload = match TaskStatePayload::build(&mut conn, task) {
        Ok(task) => task,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    Ok((warp::reply::json(&payload), session))
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

async fn update_task_handler(
    task_id: String,
    payload: TaskUpdate,
    auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
    sender: broadcast::Sender<TaskStatePayload>
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
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
    match task.update(&mut conn, auth.id(), payload) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!("Update error: {}", err);
            return Err(warp::reject::custom(DatabaseError{}));
        }
    };

    let task_state = match TaskStatePayload::build(&mut conn, task) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!("Payload build error: {}", err);
            return Err(warp::reject::custom(DatabaseError{}));
        }
    };
    sender.send(task_state.clone()).ok();
    Ok((warp::reply::json(&task_state), session))
}

async fn get_task_handler(
    task_id: String,
    _auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
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
    Ok((warp::reply::json(&task_payload), session))
}

#[derive(Deserialize)]
pub struct QueryPayload {
    page: u32,
    page_size: u32,
    query: HashMap<String, String>
}

#[derive(Serialize)]
pub struct DenormalizedTask {
    pub id: Uuid,
    pub slug: String,
    pub created: NaiveDateTime,
    pub title: String,
    pub description: String,
    pub author: DenormalizedUser,
    pub assignee: Option<DenormalizedUser>,
}

impl DenormalizedTask {
    pub fn denormalize(conn: &mut PgConnection, task: Task) -> QueryResult<Self> {
        let author = User::get(conn, task.author_id).ok_or_else(
            || diesel::result::Error::NotFound)?;
        let author = DenormalizedUser::denormalize(conn, author)?;
        let assignee = match task.assignee_id {
            Some(assignee_id) => {
                let user = User::get(conn, assignee_id).ok_or_else(|| diesel::result::Error::NotFound)?;
                Some(DenormalizedUser::denormalize(conn, user)?)
            }
            None => None
        };

        Ok(Self {
            id: task.id,
            slug: task.slug,
            created: task.created,
            title: task.title,
            description: task.description,
            author,
            assignee
        })
    }
}

#[derive(Serialize)]
pub struct QueryReply {
    tasks: Vec<DenormalizedTask>
}

async fn filter_tasks_handler(
    payload: QueryPayload,
    _auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let tasks = Task::query(&mut conn, &payload.query, payload.page, payload.page_size);
    let mut denorm_tasks = vec![];
    for task in tasks {
        let denorm_task = DenormalizedTask::denormalize(&mut conn, task)
            .map_err(|_| warp::reject::custom(DatabaseError{}))?;
        denorm_tasks.push(denorm_task);
    }

    let reply = QueryReply{tasks: denorm_tasks};
    Ok((warp::reply::json(&reply), session))
}

pub fn routes(idp: Option<Arc<IdentityProvider>>,
              session: MemoryStore,
              pool: Arc<DbPool>,
              router: &mut Router,) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let task_tx: broadcast::Sender<Task> = router.announce();
    let task_update_tx: broadcast::Sender<TaskStatePayload> = router.announce();
    let prompt_request_tx: mpsc::Sender<PromptChannel> = router.get_address()
        .expect("No prompt request channel defined").clone();

    let create_task = warp::post()
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_channel(prompt_request_tx))
        .and(with_broadcast(task_tx))
        .and_then(create_task_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let update_task = warp::put()
        .and(warp::path::param())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_update_tx))
        .and_then(update_task_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let get_task = warp::get()
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_task_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let filter_tasks = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(filter_tasks_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    warp::path("task")
        .and(filter_tasks
             .or(create_task)
             .or(update_task)
             .or(get_task))
}
