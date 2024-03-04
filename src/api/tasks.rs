use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use diesel::{PgConnection, QueryResult};
use serde::{Deserialize, Serialize};
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::oidc::IdentityProvider;
use subseq_util::{api::*, tables::{DbPool, UserTable}, Router};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::timeout;
use uuid::Uuid;
use warp::{Filter, Rejection, Reply};
use warp_sessions::{MemoryStore, SessionWithStore};

use super::prompts::{InitializePromptChannel, PromptResponseType, PromptRxPayload, PromptTx};
use super::with_channel;
use crate::api::users::DenormalizedUser;
use crate::tables::{
    ActiveProject,
    FlowConnection,
    FlowNode,
    Project,
    Task,
    TaskFlow,
    TaskLink,
    TaskLinkType,
    TaskUpdate,
    User,
};

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
    components: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InnerUpdateTaskPayload {
    user_id: Uuid,
    task_id: Uuid,
    update: TaskUpdate,
}

#[derive(Deserialize, Default)]
pub struct TitleJson {
    title: String,
}

#[derive(Deserialize, Serialize, Default)]
pub struct DescriptionJson {
    description: String,
}

async fn title_from_description(
    description: &str,
    prompt_request_tx: &mut mpsc::Sender<InitializePromptChannel>,
) -> Option<String> {
    let (request_tx, request_rx) = mpsc::channel(1);
    let (response_tx, mut response_rx) = mpsc::unbounded_channel();
    let request = PromptTx::title_from_desc(description.to_string());
    if prompt_request_tx
        .send(InitializePromptChannel(request.stream_id, request_rx, response_tx))
        .await
        .is_err()
    {
        return None;
    }
    if request_tx.send(request).await.is_err() {
        return None;
    }

    let response = match timeout(Duration::from_secs(10), response_rx.recv()).await {
        Ok(Some(response)) => response,
        Err(_) => {
            tracing::warn!("Timed out waiting for title");
            return None;
        }
        Ok(None) => {
            return None;
        }
    };

    match response {
        PromptRxPayload::Close(PromptResponseType::Json(message)) => {
            match serde_json::from_value::<TitleJson>(message) {
                Ok(title) => Some(title.title),
                Err(err) => {
                    tracing::error!("Response deserialization error: {}", err);
                    None
                }
            }
        }
        PromptRxPayload::Close(PromptResponseType::Text(message)) => Some(message),
        _ => {
            tracing::error!("Unexpecte response");
            None
        }
    }
}

pub async fn create_task(
    conn: &mut PgConnection,
    user_id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
) -> Result<Task, Rejection> {
    let mut project = match Project::get(conn, project_id) {
        Some(project) => project,
        None => return Err(warp::reject::custom(NotFoundError {})),
    };

    let user = match User::get(conn, user_id) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError {})),
    };

    Task::create(
        conn,
        Uuid::new_v4(),
        &mut project,
        &title,
        &description,
        &user,
    )
    .map_err(|err| {
        tracing::error!("Task creation failed: {:?}", err);
        warp::reject::custom(ConflictError {})
    })
}

async fn create_task_handler(
    payload: TaskPayload,
    auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
    mut prompt_request_tx: mpsc::Sender<InitializePromptChannel>,
    sender: broadcast::Sender<Task>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    // We explicitly ignore setting the task id from the api.
    let TaskPayload {
        project_id,
        title,
        description,
    } = payload;
    let title = if let Some(title) = title {
        title
    } else {
        title_from_description(&description, &mut prompt_request_tx)
            .await
            .ok_or_else(|| warp::reject::custom(InvalidConfigurationError {}))?
    };
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let task = create_task(&mut conn, auth.id(), project_id, title, description).await?;
    sender.send(task.clone()).ok();

    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let payload = match TaskStatePayload::build(&mut conn, task) {
        Ok(task) => task,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    Ok((warp::reply::json(&payload), session))
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatePayload {
    pub task: Task,
    pub tags: Vec<String>,
    pub watchers: Vec<User>,
    pub state: FlowNode,
    pub links_out: Vec<TaskLink>,
    pub links_in: Vec<TaskLink>,
    pub valid_transitions: Vec<FlowNode>,
}

impl TaskStatePayload {
    pub fn build(conn: &mut PgConnection, task: Task) -> QueryResult<Self> {
        let flows = task.flows(conn)?;
        let tags = task.tags(conn).ok().unwrap_or_default();
        let watchers = task.watchers(conn).ok().unwrap_or_default();
        let state = TaskFlow::get_active_node(conn, &flows)?;
        let valid_transitions = FlowConnection::edges(conn, state.id)?;
        let links_out = TaskLink::get_outgoing(conn, &task)?;
        let links_in = TaskLink::get_incoming(conn, &task)?;
        Ok(Self {
            task,
            tags,
            watchers,
            state,
            links_out,
            links_in,
            valid_transitions,
        })
    }
}

pub async fn update_task(
    conn: &mut PgConnection,
    user_id: Uuid,
    task_id: Uuid,
    update: TaskUpdate,
) -> Result<Task, Rejection> {
    let mut task = match Task::get(conn, task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError {}));
        }
    };
    match task.update(conn, user_id, update) {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!("Update error: {}", err);
            return Err(warp::reject::custom(DatabaseError {}));
        }
    };
    Ok(task)
}

async fn update_task_handler(
    task_id: Uuid,
    payload: TaskUpdate,
    auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
    sender: broadcast::Sender<TaskStatePayload>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let task = update_task(&mut conn, auth.id(), task_id, payload).await?;
    let task_denorm = DenormalizedTask::denormalize(&mut conn, &task)
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;
    let task_state = TaskStatePayload::build(&mut conn, task)
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;
    sender.send(task_state).ok();
    Ok((warp::reply::json(&task_denorm), session))
}

#[derive(Serialize, Deserialize)]
pub struct GetTaskQuery {
    denormalized: bool,
}

async fn get_task_handler(
    task_id: String,
    query: GetTaskQuery,
    _auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let task_id = match Uuid::parse_str(&task_id) {
        Ok(task_id) => task_id,
        Err(_) => {
            return Err(warp::reject::custom(ParseError {}));
        }
    };
    let task = match Task::get(&mut conn, task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError {}));
        }
    };

    if query.denormalized {
        let task_details = DenormalizedTaskDetails::denormalize(&mut conn, &task)
            .map_err(|_| warp::reject::custom(DatabaseError {}))?;
        Ok((warp::reply::json(&task_details), session))
    } else {
        let task_payload = match TaskStatePayload::build(&mut conn, task) {
            Ok(state) => state,
            Err(_) => return Err(warp::reject::custom(DatabaseError {})),
        };
        Ok((warp::reply::json(&task_payload), session))
    }
}

#[derive(Deserialize)]
pub struct QueryPayload {
    pub page: u32,
    pub page_size: u32,
    pub query: HashMap<String, String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct DenormalizedTask {
    pub id: Uuid,
    pub slug: String,
    pub created: NaiveDateTime,
    pub title: String,
    pub description: String,
    pub author: DenormalizedUser,
    pub assignee: Option<DenormalizedUser>,
    pub open: bool
}

impl DenormalizedTask {
    pub fn denormalize(conn: &mut PgConnection, task: &Task) -> QueryResult<Self> {
        let author =
            User::get(conn, task.author_id).ok_or_else(|| diesel::result::Error::NotFound)?;
        let author = DenormalizedUser::denormalize(conn, author)?;
        let assignee = match task.assignee_id {
            Some(assignee_id) => {
                let user =
                    User::get(conn, assignee_id).ok_or_else(|| diesel::result::Error::NotFound)?;
                Some(DenormalizedUser::denormalize(conn, user)?)
            }
            None => None,
        };
        let flows = task.flows(conn)?;
        let state = TaskFlow::get_active_node(conn, &flows)?;
        let valid_transitions = FlowConnection::edges(conn, state.id)?;

        Ok(Self {
            id: task.id,
            slug: task.slug.clone(),
            created: task.created,
            title: task.title.clone(),
            description: task.description.clone(),
            author,
            assignee,
            open: !valid_transitions.is_empty(),
        })
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct DenormalizedTaskLink {
    pub link: DenormalizedTask,
    pub link_type: TaskLinkType,
}

#[derive(Serialize, Debug, Clone)]
pub enum DirectionalLink {
    From(TaskLink),
    To(TaskLink),
}

impl DenormalizedTaskLink {
    fn denormalize(conn: &mut PgConnection, link: DirectionalLink) -> QueryResult<Self> {
        let (task_id, link_type) = match link {
            DirectionalLink::From(task_link) => (task_link.task_from_id, task_link.link_type),
            DirectionalLink::To(task_link) => (task_link.task_to_id, task_link.link_type),
        };
        let task = Task::get_result(conn, task_id)?;
        let task_deno = DenormalizedTask::denormalize(conn, &task)?;

        Ok(Self {
            link: task_deno,
            link_type,
        })
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct DenormalizedTaskDetails {
    pub tags: Vec<String>,
    pub watchers: Vec<DenormalizedUser>,
    pub state: FlowNode,
    pub links_out: Vec<DenormalizedTaskLink>,
    pub links_in: Vec<DenormalizedTaskLink>,
    pub valid_transitions: Vec<FlowNode>,
}

impl DenormalizedTaskDetails {
    pub fn denormalize(conn: &mut PgConnection, task: &Task) -> QueryResult<Self> {
        let flows = task.flows(conn)?;
        let tags = task.tags(conn).ok().unwrap_or_default();

        let watchers: Vec<DenormalizedUser> = task
            .watchers(conn)?
            .into_iter()
            .filter_map(|user| DenormalizedUser::denormalize(conn, user).ok())
            .collect();
        let state = TaskFlow::get_active_node(conn, &flows)?;
        let valid_transitions = FlowConnection::edges(conn, state.id)?;

        let links_out: Vec<DenormalizedTaskLink> = TaskLink::get_outgoing(conn, task)?
            .into_iter()
            .filter_map(|task_link| {
                DenormalizedTaskLink::denormalize(conn, DirectionalLink::To(task_link)).ok()
            })
            .collect();
        let links_in: Vec<DenormalizedTaskLink> = TaskLink::get_incoming(conn, task)?
            .into_iter()
            .filter_map(|task_link| {
                DenormalizedTaskLink::denormalize(conn, DirectionalLink::From(task_link)).ok()
            })
            .collect();

        Ok(Self {
            tags,
            watchers,
            state,
            links_out,
            links_in,
            valid_transitions,
        })
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct DenormalizedTaskState {
    pub submitted_by: DenormalizedUser,
    pub active_project: Uuid,
    pub task: DenormalizedTask,
    pub details: DenormalizedTaskDetails,
}

impl DenormalizedTaskState {
    pub fn denormalize(conn: &mut PgConnection,
                       submitted_by: User,
                       active_project: Uuid,
                       task: &Task) -> QueryResult<Self> {
        Ok(Self {
            submitted_by: DenormalizedUser::denormalize(conn, submitted_by)?,
            active_project,
            task: DenormalizedTask::denormalize(conn, task)?,
            details: DenormalizedTaskDetails::denormalize(conn, task)?,
        })
    }
}

#[derive(Serialize)]
pub struct QueryReply {
    pub tasks: Vec<DenormalizedTask>,
}

pub type QueryChannel = (QueryPayload, oneshot::Sender<Result<QueryReply, Rejection>>);

pub async fn filter_tasks(
    auth: AuthenticatedUser,
    conn: &mut PgConnection,
    payload: QueryPayload,
) -> Result<QueryReply, Rejection> {
    let tasks = Task::query(
        conn,
        auth.id(),
        &payload.query,
        payload.page,
        payload.page_size,
    );
    let mut denorm_tasks = vec![];
    for task in tasks {
        let denorm_task = DenormalizedTask::denormalize(conn, &task)
            .map_err(|_| warp::reject::custom(DatabaseError {}))?;
        denorm_tasks.push(denorm_task);
    }

    Ok(QueryReply {
        tasks: denorm_tasks,
    })
}

async fn filter_tasks_handler(
    payload: QueryPayload,
    auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    Ok((
        warp::reply::json(&filter_tasks(auth, &mut conn, payload).await?),
        session,
    ))
}

#[derive(Clone, Debug)]
pub struct TaskRun {
    pub state: DenormalizedTaskState,
}

async fn run_task_handler(
    task_id: Uuid,
    auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
    task_run_tx: broadcast::Sender<TaskRun>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let task = match Task::get(&mut conn, task_id) {
        Some(task) => task,
        None => {
            return Err(warp::reject::custom(NotFoundError {}));
        }
    };
    if task.assignee_id.is_none() {
        return Err(warp::reject::custom(InvalidConfigurationError {}));
    }
    let submitted_user = User::get(&mut conn, auth.id())
        .ok_or_else(|| warp::reject::custom(NotFoundError {}))?;
    let active_project = ActiveProject::get(&mut conn, submitted_user.id)
        .ok_or_else(|| warp::reject::custom(NotFoundError {}))?;
    let state = DenormalizedTaskState::denormalize(&mut conn,
                                                   submitted_user,
                                                   active_project.project_id,
                                                   &task)
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;

    let run = TaskRun { state };
    task_run_tx.send(run).ok();

    Ok((
        warp::reply::json(&serde_json::json!({"run": "started"})),
        session,
    ))
}

pub fn routes(
    idp: Option<Arc<IdentityProvider>>,
    session: MemoryStore,
    pool: Arc<DbPool>,
    router: &mut Router,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let task_tx: broadcast::Sender<Task> = router.announce();
    let task_update_tx: broadcast::Sender<TaskStatePayload> = router.announce();
    let task_run_tx: broadcast::Sender<TaskRun> = router.announce();
    let prompt_request_tx: mpsc::Sender<InitializePromptChannel> = router
        .get_address()
        .expect("No prompt request channel defined")
        .clone();

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
        .and(warp::query::<GetTaskQuery>())
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

    let run_task = warp::path("run")
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(task_run_tx))
        .and_then(run_task_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    warp::path("task").and(
        filter_tasks
            .or(create_task)
            .or(update_task)
            .or(run_task)
            .or(get_task),
    )
}
