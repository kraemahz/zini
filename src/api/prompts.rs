use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use subseq_util::api::AuthenticatedUser;
use subseq_util::tables::{DbPool, UserTable};
use subseq_util::Router;
use tokio::{sync::{broadcast, mpsc}, select, task::spawn};
use uuid::Uuid;

use super::tasks::{create_task, filter_tasks, update_task, QueryPayload};
use super::users::DenormalizedUser;
use crate::interop::JobRequestType;
use crate::tables::{ActiveProject, Flow, Project, Task, TaskLinkType, TaskUpdate, User};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    Request,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    Text(String),
    Object(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletion {
    pub role: ChatRole,
    pub content: Content,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptTx {
    pub stream_id: Uuid,
    pub payload: PromptTxPayload,
}

impl PromptTx {
    pub fn title_from_desc(desc: String) -> Self {
        let prompt_id = "tools_aNDkkK".to_string();
        Self {
            stream_id: Uuid::new_v4(),
            payload: PromptTxPayload::Handshake {
                prompt_id,
                prompt_start: desc,
            },
        }
    }

    pub fn new_stream(prompt_start: String) -> Self {
        let prompt_id = "tools_ZbTYeQ".to_string();
        Self {
            stream_id: Uuid::new_v4(),
            payload: PromptTxPayload::Handshake {
                prompt_id,
                prompt_start,
            },
        }
    }

    pub fn stream_update(stream_id: Uuid, update: String) -> Self {
        Self {
            stream_id,
            payload: PromptTxPayload::Stream(update),
        }
    }

    pub fn tool_result(stream_id: Uuid, tool_result: ToolResult) -> Self {
        Self {
            stream_id,
            payload: PromptTxPayload::ToolResult(tool_result),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptRx {
    pub stream_id: Uuid,
    pub payload: PromptRxPayload,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskSummary {
    task_id: Uuid,
    title: String,
    description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ToolResult {
    RunTask(Uuid),
    CreateTask(TaskSummary),
    UpdateTask(TaskSummary),
    FetchTasks(Vec<TaskSummary>),
    BeginProject { project_id: Uuid },
    Error(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PromptTxPayload {
    Handshake {
        prompt_id: String,
        prompt_start: String,
    },
    ToolResult(ToolResult),
    Stream(String),
    HelpResponse,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Tool {
    RunTask {
        task_id: Uuid,
    },
    CreateTask {
        title: String,
        description: String,
        subtask_of: Option<String>,
        blocked_by: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        components: Vec<String>,
    },
    UpdateTask {
        task_id: Uuid,
        update: TaskUpdate,
    },
    FetchTasks,
    BeginProject {
        title: String,
        description: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PromptResponseType {
    Text(String),
    Json(Value),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PromptRxPayload {
    Tool(Tool),
    Stream {
        update: String,
        response_expected: bool,
    },
    JobRequest(JobRequestType, User),
    Close(PromptResponseType),
}

pub struct InitializePromptChannel (
    pub Uuid,
    pub mpsc::Receiver<PromptTx>,
    pub mpsc::UnboundedSender<PromptRxPayload>,
);

#[derive(Clone, Debug)]
pub struct PromptChannelHandle {
    user_to_prompt: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<PromptRxPayload>>>>,
}

impl PromptChannelHandle {
    pub fn new() -> Self {
        Self {
            user_to_prompt: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    pub fn add_channel_for_user(&self, user_id: Uuid) -> (mpsc::UnboundedSender<PromptRxPayload>, mpsc::UnboundedReceiver<PromptRxPayload>) {
        let mut map = self.user_to_prompt.lock().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        map.insert(user_id, tx.clone());
        (tx, rx)
    }

    pub fn drop_channel_for_user(&self, user_id: Uuid) {
        let mut map = self.user_to_prompt.lock().unwrap();
        map.remove(&user_id);
    }

    pub fn get_user_tx(&self, user_id: Uuid) -> Option<mpsc::UnboundedSender<PromptRxPayload>> {
        let map = self.user_to_prompt.lock().unwrap();
        map.get(&user_id).map(|tx| tx.clone())
    }
}

#[derive(Clone, Debug)]
pub struct PromptResponseCollection {
    inner: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<PromptRxPayload>>>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UIRequest {
    user: DenormalizedUser,
    request: JobRequestType
}

impl PromptResponseCollection {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert(&self, request_id: Uuid, sender: mpsc::UnboundedSender<PromptRxPayload>) {
        self.inner.lock().unwrap().insert(request_id, sender);
    }

    pub fn send_response(&self, prompt_response: PromptRx) {
        let PromptRx { stream_id, payload } = prompt_response;
        match &payload {
            PromptRxPayload::Close(_) => {
                let sender = self.inner.lock().unwrap().remove(&stream_id);
                if let Some(sender) = sender {
                    sender.send(payload).ok();
                }
            }
            _ => {
                let inner = self.inner.lock().unwrap();
                let sender = inner.get(&stream_id);
                if let Some(sender) = sender {
                    sender.send(payload).ok();
                }
            }
        }
    }
}

impl Default for PromptResponseCollection {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum AuthRequestPayload {
    Task {
        title: String,
        description: String,
        components: Vec<String>,
    },
    TaskUpdate {
        task_id: Uuid,
        update: TaskUpdate,
    },
    Project {
        title: String,
        description: String,
    },
}

#[derive(Debug)]
pub struct InstructionState {
    auth_user: AuthenticatedUser,
    assignee: Option<User>,
    stream_id: Option<Uuid>,
    project_id: Uuid,
    check_auth: bool
}

impl InstructionState {
    async fn run_tool(
        &mut self,
        tool: Tool,
        conn: &mut PgConnection,
        connections: Connections<'_>,
        string_rx: &mut mpsc::Receiver<String>,
    ) -> ToolResult {
        match tool {
            Tool::RunTask { task_id: _ } => {
                // TODO: This currently doesn't have an API
                ToolResult::RunTask(Uuid::new_v4())
            }
            Tool::FetchTasks => {
                let mut query = HashMap::new();
                query.insert(String::from("project"), String::from("active"));
                let payload = QueryPayload {
                    page: 0,
                    page_size: 50,
                    query,
                };
                let result = filter_tasks(self.auth_user, conn, payload).await;
                match result {
                    Ok(reply) => {
                        let summary_vec: Vec<_> = reply
                            .tasks
                            .into_iter()
                            .map(|task| TaskSummary {
                                task_id: task.id,
                                title: task.title,
                                description: task.description,
                            })
                            .collect();
                        ToolResult::FetchTasks(summary_vec)
                    }
                    Err(err) => ToolResult::Error(format!("Tool error: {:?}", err)),
                }
            }
            Tool::CreateTask {
                title,
                description,
                subtask_of,
                blocked_by,
                tags,
                components,
            } => {
                let authed_task = AuthRequestPayload::Task {
                    title: title.clone(),
                    description: description.clone(),
                    components: components.clone(),
                };
                if self.check_auth && !connections.auth_request(string_rx, authed_task).await {
                    return ToolResult::Error("Change was rejected by the user".to_string());
                }

                let mut task = match create_task(
                    conn,
                    self.auth_user.id(),
                    self.project_id,
                    title,
                    description,
                )
                .await
                {
                    Ok(task) => task,
                    Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
                };

                if let Some(subtask_of) = subtask_of {
                    if let Ok(uuid) = Uuid::parse_str(&subtask_of) {
                        task.add_link(conn, uuid, TaskLinkType::SubtaskOf).ok();
                    }
                }

                if let Some(blocked_by) = blocked_by {
                    for block_id in blocked_by {
                        if let Ok(uuid) = Uuid::parse_str(&block_id) {
                            task.add_link(conn, uuid, TaskLinkType::DependsOn).ok();
                        }
                    }
                }

                if let Some(tags) = tags {
                    for tag in tags {
                        let label = serde_json::to_string(&serde_json::json!({"label": tag}))
                            .expect("is valid");
                        task.add_tag(conn, &label).ok();
                    }
                }
                for component in components {
                    let component = serde_json::to_string(&serde_json::json!({"component": component}))
                        .expect("is valid");
                    task.add_tag(conn, &component).ok();
                }

                if let Some(user) = self.assignee.as_ref() {
                    task.update(conn, self.auth_user.id(),
                                TaskUpdate::AssignOther { user_id: user.id }).ok();
                }

                connections.task_tx.send(task.clone()).ok();
                ToolResult::CreateTask(TaskSummary {
                    task_id: task.id,
                    title: task.title,
                    description: task.description,
                })
            }
            Tool::UpdateTask { task_id, update } => {
                let authed_task = AuthRequestPayload::TaskUpdate {
                    task_id,
                    update: update.clone(),
                };
                if self.check_auth && !connections.auth_request(string_rx, authed_task).await {
                    return ToolResult::Error("Change was rejected by the user".to_string());
                }
                let task = match update_task(conn, self.auth_user.id(), task_id, update).await {
                    Ok(task) => task,
                    Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
                };
                ToolResult::UpdateTask(TaskSummary {
                    task_id: task.id,
                    title: task.title,
                    description: task.description,
                })
            }
            Tool::BeginProject { title, description } => {
                let authed_project = AuthRequestPayload::Project {
                    title: title.clone(),
                    description: description.clone(),
                };
                if self.check_auth && !connections.auth_request(string_rx, authed_project).await {
                    return ToolResult::Error("Change was rejected by the user".to_string());
                }
                let user = match User::get(conn, self.auth_user.id()) {
                    Some(user) => user,
                    None => return ToolResult::Error("Database error: missing user".to_string()),
                };

                let flow = match Flow::list(conn, 0, 1).into_iter().next() {
                    Some(flow) => flow,
                    None => return ToolResult::Error("No flows defined in database".to_string()),
                };

                let project = match Project::create(
                    conn,
                    Uuid::new_v4(),
                    &user,
                    &title.to_ascii_uppercase(),
                    &description,
                    &flow,
                ) {
                    Ok(project) => project,
                    Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
                };
                if project.set_active_project(conn, user.id).is_err() {
                    return ToolResult::Error("Could not set the active project".to_string());
                }
                connections.project_tx.send(project.clone()).ok();

                self.project_id = project.id;
                self.check_auth = false; // When the project is set to a new one we can ignore auth for
                                         // the rest of the project.
                ToolResult::BeginProject {
                    project_id: project.id,
                }
            }
        }
    }
}


#[derive(Clone, Copy, Debug)]
pub struct Connections<'a> {
    chat_tx: &'a mpsc::Sender<ChatCompletion>,
    project_tx: &'a broadcast::Sender<Project>,
    task_tx: &'a broadcast::Sender<Task>,
    prompt_tx: &'a mpsc::Sender<PromptTx>,
}


impl<'a> Connections<'a> {
    async fn auth_request(
        &self,
        string_rx: &mut mpsc::Receiver<String>,
        auth_request: AuthRequestPayload,
    ) -> bool {
        let chat = ChatCompletion {
            role: ChatRole::System,
            content: Content::Object(serde_json::to_value(&auth_request).expect("Request")),
        };
        if self.chat_tx.send(chat).await.is_err() {
            return false;
        }
        let auth_response = match string_rx.recv().await {
            Some(auth) => auth,
            None => return false,
        };
        for element in auth_response.split_ascii_whitespace() {
            if element.to_ascii_lowercase().as_str() == "accept" {
                return true;
            }
        }
        false
    }
}

fn reassign_user(conn: &mut PgConnection,
                 update: &str) -> Option<Option<User>> {
    if update.contains("FrontendManager") {
        return Some(User::from_username(conn, "FRONTEND"));
    }
    if update.contains("BackendManager") {
        return Some(User::from_username(conn, "BACKEND"));
    }
    if update.contains("DevopsManager") {
        return Some(User::from_username(conn, "DEVOPS"));
    }
    // Don't want to set assignee on single tasks
    if update.contains("Creating task") {
        return Some(None);
    }
    None
}

async fn instruction_channel_message(response: PromptRxPayload,
                                     conn: &mut PgConnection,
                                     connections: Connections<'_>,
                                     state: &mut InstructionState,
                                     string_rx: &mut mpsc::Receiver<String>) -> Option<bool> {
    match response {
        PromptRxPayload::Tool(tool) => {
            let stream_id = state.stream_id?;
            let tool_response = state.run_tool(
                tool,
                conn,
                connections,
                string_rx,
            )
            .await;
            let response = PromptTx::tool_result(stream_id, tool_response);
            connections.prompt_tx.send(response).await.ok()?;
        }
        PromptRxPayload::Stream { update, response_expected } => {
            // HACKY
            if let Some(new_assignee) = reassign_user(conn, &update) {
                state.assignee = new_assignee;
            }

            let chat = ChatCompletion {
                role: ChatRole::Assistant,
                content: Content::Text(update),
            };
            connections.chat_tx.send(chat).await.ok()?;

            if response_expected {
                let stream_id = state.stream_id?;
                let text = string_rx.recv().await?;
                connections.chat_tx
                    .send(ChatCompletion {
                        role: ChatRole::User,
                        content: Content::Text(text.clone()),
                    })
                    .await
                    .ok();
                let response = PromptTx::stream_update(stream_id, text);
                connections.prompt_tx.send(response).await.ok()?;
            }
        }
        PromptRxPayload::JobRequest(job_request_type, user) => {
            let denorm_user = DenormalizedUser::denormalize(conn, user).ok()?;
            let request = UIRequest { user: denorm_user,
                                      request: job_request_type };
            let chat = ChatCompletion {
                role: ChatRole::Request,
                content: Content::Object(serde_json::to_value(&request).ok()?),
            };
            connections.chat_tx.send(chat).await.ok()?;
        }
        PromptRxPayload::Close(last_update) => {
            let chat = match last_update {
                PromptResponseType::Json(json) => ChatCompletion {
                    role: ChatRole::System,
                    content: Content::Object(serde_json::to_value(&json).ok()?),
                },
                PromptResponseType::Text(text) => ChatCompletion {
                    role: ChatRole::Assistant,
                    content: Content::Text(text),
                },
            };
            connections.chat_tx.send(chat).await.ok()?;

            let chat = ChatCompletion {
                role: ChatRole::System,
                content: Content::Object(serde_json::json!({"state": "closed"})),
            };
            connections.chat_tx.send(chat).await.ok()?;
            return Some(true);
        }
    }
    Some(false)
}

async fn new_instruction_channel(
    db_pool: Arc<DbPool>,
    auth_user: AuthenticatedUser,
    mut string_rx: mpsc::Receiver<String>,
    chat_tx: mpsc::Sender<ChatCompletion>,
    project_tx: broadcast::Sender<Project>,
    task_tx: broadcast::Sender<Task>,
    prompt_channel: PromptChannelHandle,
    initialize_prompt_tx: mpsc::Sender<InitializePromptChannel>,
) -> Option<()> {
    tracing::info!("New instruction channel");

    loop {
        let (prompt_tx, prompt_request_rx) = mpsc::channel(64);
        let connections = Connections {
            chat_tx: &chat_tx,
            project_tx: &project_tx,
            task_tx: &task_tx,
            prompt_tx: &prompt_tx,
        };

        let project_id = {
            let mut conn = db_pool.get().ok()?;
            ActiveProject::get(&mut conn, auth_user.id())?.project_id
        };
        let mut state = InstructionState {
            auth_user,
            assignee: None,
            stream_id: None,
            project_id,
            check_auth: true,
        };
        let (prompt_response_tx, mut prompt_rx) = prompt_channel.add_channel_for_user(auth_user.id());

        // The inital ask from the instruct stream
        let initial_request = loop {
            select! {
                msg = prompt_rx.recv() => {
                    let response = msg?;
                    let mut conn = db_pool.get().ok()?;
                    instruction_channel_message(response,
                                                &mut conn,
                                                connections,
                                                &mut state,
                                                &mut string_rx).await?;
                }
                msg = string_rx.recv() => {
                    let msg = msg?;
                    break msg;
                }
            }
        };

        chat_tx
            .send(ChatCompletion {
                role: ChatRole::User,
                content: Content::Text(initial_request.clone()),
            })
            .await
            .ok();
        tracing::info!("Initial request {}", initial_request);
        let handshake = PromptTx::new_stream(initial_request);

        let stream_id = handshake.stream_id;
        initialize_prompt_tx
            .send(InitializePromptChannel(stream_id, prompt_request_rx, prompt_response_tx))
            .await
            .ok()?;
        prompt_tx.send(handshake).await.ok()?;

        loop {
            let response = prompt_rx.recv().await?;
            let mut conn = db_pool.get().ok()?;
            tracing::info!("Prompt rx {:?}", response);
            if instruction_channel_message(response,
                                           &mut conn,
                                           connections,
                                           &mut state,
                                           &mut string_rx).await? {
                break;
            }
        }
    }
}

pub struct InstructChannel(
    pub AuthenticatedUser,
    pub mpsc::Receiver<String>,
    pub mpsc::Sender<ChatCompletion>,
);

pub fn instruction_channel_task(db_pool: Arc<DbPool>, router: &mut Router, prompt_channel: PromptChannelHandle) {
    let mut instruction_config_rx: mpsc::Receiver<InstructChannel> = router.create_channel();
    let prompt_request_tx: mpsc::Sender<InitializePromptChannel> =
        router.get_address().expect("Could't get address").clone();
    let project_tx = router.announce();
    let task_tx = router.announce();

    spawn(async move {
        while let Some(InstructChannel(auth_user, rx, tx)) = instruction_config_rx.recv().await {
            spawn(new_instruction_channel(db_pool.clone(),
                                          auth_user,
                                          rx,
                                          tx,
                                          project_tx.clone(),
                                          task_tx.clone(),
                                          prompt_channel.clone(),
                                          prompt_request_tx.clone(),
            ));
        }
    });
}
