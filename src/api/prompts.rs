use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use subseq_util::api::AuthenticatedUser;
use subseq_util::tables::{DbPool, UserTable};
use subseq_util::Router;
use tokio::{sync::mpsc, task::spawn};
use uuid::Uuid;

use super::tasks::{create_task, filter_tasks, update_task, QueryPayload};
use crate::tables::{ActiveProject, Flow, Project, TaskLinkType, TaskUpdate, User};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
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
    Close(PromptResponseType),
}

pub type InitializePromptChannel = (
    Uuid,
    mpsc::Receiver<PromptTx>,
    mpsc::UnboundedSender<PromptRxPayload>,
);

#[derive(Clone, Debug)]
pub struct PromptResponseCollection {
    inner: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<PromptRxPayload>>>>,
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

async fn auth_request(
    string_rx: &mut mpsc::Receiver<String>,
    chat_tx: &mpsc::Sender<ChatCompletion>,
    auth_request: AuthRequestPayload,
) -> bool {
    let chat = ChatCompletion {
        role: ChatRole::System,
        content: Content::Object(serde_json::to_value(&auth_request).expect("Request")),
    };
    if chat_tx.send(chat).await.is_err() {
        return false;
    }
    let auth_response = match string_rx.recv().await {
        Some(auth) => auth,
        None => return false,
    };
    for element in auth_response.split_ascii_whitespace() {
        match element.to_ascii_lowercase().as_str() {
            "accept" => return true,
            _ => {}
        }
    }
    false
}

async fn run_tool(
    db_pool: Arc<DbPool>,
    string_rx: &mut mpsc::Receiver<String>,
    chat_tx: &mpsc::Sender<ChatCompletion>,
    auth_user: AuthenticatedUser,
    project_id: Uuid,
    check_auth: &mut bool,
    tool: Tool,
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
            let result = filter_tasks(auth_user, db_pool, payload).await;
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
            if *check_auth && !auth_request(string_rx, chat_tx, authed_task).await {
                return ToolResult::Error("Change was rejected by the user".to_string());
            }

            let task = match create_task(
                db_pool.clone(),
                auth_user.id(),
                project_id,
                title,
                description,
            )
            .await
            {
                Ok(task) => task,
                Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
            };

            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(err) => return ToolResult::Error(err.to_string()),
            };

            if let Some(subtask_of) = subtask_of {
                if let Ok(uuid) = Uuid::parse_str(&subtask_of) {
                    task.add_link(&mut conn, uuid, TaskLinkType::SubtaskOf).ok();
                }
            }

            if let Some(blocked_by) = blocked_by {
                for block_id in blocked_by {
                    if let Ok(uuid) = Uuid::parse_str(&block_id) {
                        task.add_link(&mut conn, uuid, TaskLinkType::DependsOn).ok();
                    }
                }
            }

            if let Some(tags) = tags {
                for tag in tags {
                    let label = serde_json::to_string(&serde_json::json!({"label": tag}))
                        .expect("is valid");
                    task.add_tag(&mut conn, &label).ok();
                }
            }
            for component in components {
                let component = serde_json::to_string(&serde_json::json!({"component": component}))
                    .expect("is valid");
                task.add_tag(&mut conn, &component).ok();
            }
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
            if *check_auth && !auth_request(string_rx, chat_tx, authed_task).await {
                return ToolResult::Error("Change was rejected by the user".to_string());
            }
            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => return ToolResult::Error(format!("Database access error")),
            };
            let task = match update_task(&mut conn, auth_user.id(), task_id, update).await {
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
            if *check_auth && !auth_request(string_rx, chat_tx, authed_project).await {
                return ToolResult::Error("Change was rejected by the user".to_string());
            }

            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
            };

            let user = match User::get(&mut conn, auth_user.id()) {
                Some(user) => user,
                None => return ToolResult::Error("Database error: missing user".to_string()),
            };

            let flow = match Flow::list(&mut conn, 0, 1).into_iter().next() {
                Some(flow) => flow,
                None => return ToolResult::Error("No flows defined in database".to_string()),
            };

            let project = match Project::create(
                &mut conn,
                &user,
                &title.to_ascii_uppercase(),
                &description,
                &flow,
            ) {
                Ok(project) => project,
                Err(err) => return ToolResult::Error(format!("Database error: {:?}", err)),
            };
            if project.set_active_project(&mut conn, user.id).is_err() {
                return ToolResult::Error("Could not set the active project".to_string());
            }
            *check_auth = false; // When the project is set to a new one we can ignore auth for
                                 // the rest of the project.
            ToolResult::BeginProject {
                project_id: project.id,
            }
        }
    }
}

async fn new_instruction_channel(
    db_pool: Arc<DbPool>,
    auth_user: AuthenticatedUser,
    mut string_rx: mpsc::Receiver<String>,
    chat_tx: mpsc::Sender<ChatCompletion>,
    initialize_prompt_tx: mpsc::Sender<InitializePromptChannel>,
) -> Option<()> {
    tracing::info!("New instruction channel");

    loop {
        // The inital ask from the instruct stream
        let initial_request = string_rx.recv().await?;
        let mut check_auth = true;

        let (prompt_tx, prompt_request_rx) = mpsc::channel(64);
        let (prompt_response_tx, mut prompt_rx) = mpsc::unbounded_channel();

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
            .send((stream_id, prompt_request_rx, prompt_response_tx))
            .await
            .ok()?;
        prompt_tx.send(handshake).await.ok()?;

        let mut conn = db_pool.get().ok()?;
        loop {
            let response = prompt_rx.recv().await?;
            let project_id = ActiveProject::get(&mut conn, auth_user.id())?.project_id;
            tracing::info!("Prompt rx {:?}", response);
            match response {
                PromptRxPayload::Tool(tool) => {
                    let tool_response = run_tool(
                        db_pool.clone(),
                        &mut string_rx,
                        &chat_tx,
                        auth_user,
                        project_id,
                        &mut check_auth,
                        tool,
                    )
                    .await;
                    let response = PromptTx::tool_result(stream_id, tool_response);
                    prompt_tx.send(response).await.ok()?;
                }
                PromptRxPayload::Stream {
                    update,
                    response_expected,
                } => {
                    let chat = ChatCompletion {
                        role: ChatRole::Assistant,
                        content: Content::Text(update),
                    };
                    chat_tx.send(chat).await.ok()?;

                    if response_expected {
                        let text = string_rx.recv().await?;
                        chat_tx
                            .send(ChatCompletion {
                                role: ChatRole::User,
                                content: Content::Text(text.clone()),
                            })
                            .await
                            .ok();
                        let response = PromptTx::stream_update(stream_id, text);
                        prompt_tx.send(response).await.ok()?;
                    }
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
                    chat_tx.send(chat).await.ok()?;

                    let chat = ChatCompletion {
                        role: ChatRole::System,
                        content: Content::Object(serde_json::json!({"state": "closed"})),
                    };
                    chat_tx.send(chat).await.ok()?;
                    break;
                }
            }
        }
    }
}

pub type InstructChannel = (
    AuthenticatedUser,
    mpsc::Receiver<String>,
    mpsc::Sender<ChatCompletion>,
);

pub fn instruction_channel_task(db_pool: Arc<DbPool>, router: &mut Router) {
    let mut instruction_config_rx: mpsc::Receiver<InstructChannel> = router.create_channel();
    let prompt_request_tx: mpsc::Sender<InitializePromptChannel> =
        router.get_address().expect("Could't get address").clone();
    spawn(async move {
        while let Some((auth_user, rx, tx)) = instruction_config_rx.recv().await {
            spawn(new_instruction_channel(
                db_pool.clone(),
                auth_user,
                rx,
                tx,
                prompt_request_tx.clone(),
            ));
        }
    });
}
