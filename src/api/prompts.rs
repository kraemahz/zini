use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use subseq_util::Router;
use subseq_util::api::{AuthenticatedUser, authenticate};
use subseq_util::oidc::IdentityProvider;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
use warp::Filter;
use warp::{Reply, reject::Rejection};
use warp_sessions::{MemoryStore, SessionWithStore};

use super::with_channel;


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub prompt_id: String,
    pub prompt: String
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PromptResponseType {
    Text(String),
    Json(Value)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptResponse {
    pub request_id: Uuid,
    pub response: Option<PromptResponseType>,
    pub error: Option<String>
}

pub type PromptChannel = (PromptRequest, oneshot::Sender<PromptResponse>);

#[derive(Clone, Debug)]
pub struct PromptResponseCollection {
    inner: Arc<Mutex<HashMap<Uuid, oneshot::Sender<PromptResponse>>>>
}

impl PromptResponseCollection {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn insert(&self, request_id: Uuid, sender: oneshot::Sender<PromptResponse>) {
        self.inner.lock().unwrap().insert(request_id, sender);
    }

    pub fn send_response(&self, prompt_response: PromptResponse) {
        let request_id = &prompt_response.request_id;
        if let Some(sender) = self.inner.lock().unwrap().remove(request_id) {
            sender.send(prompt_response).ok();
        }
    }
}

#[derive(Deserialize)]
struct ArchitectPayload {
    project_id: Uuid,
    requirements: Vec<String>,
    technology: Vec<String>
}

fn build_architect_prompt(payload: &ArchitectPayload) -> String {
    const REQUIREMENT: &str = "[REQUIREMENT]";
    const TECHNOLOGY: &str = "[TECHNOLOGY]";
    let mut architect_prompt = String::new();
    for req in payload.requirements.iter() {
        let req_str = format!("{}\n{}\n", REQUIREMENT, req);
        architect_prompt.push_str(&req_str);
    }
    for tech in payload.technology.iter() {
        let tech_str = format!("{}\n{}\n", TECHNOLOGY, tech);
        architect_prompt.push_str(&tech_str);
    }
    architect_prompt
}

async fn test_architect_handler(
    payload: ArchitectPayload,
    auth: AuthenticatedUser,
    _session: SessionWithStore<MemoryStore>,
    prompt_request_tx: mpsc::Sender<PromptChannel>) -> Result<impl Reply, Rejection> {

    const ARCHITECT_PROMPT_ID: &str = "architect_BKpM78";
    let prompt = build_architect_prompt(&payload);
    let request_id = Uuid::new_v4();

    let request = PromptRequest {
        request_id,
        project_id: payload.project_id,
        user_id: auth.id(),
        prompt_id: ARCHITECT_PROMPT_ID.to_string(),
        prompt
    };
    let (tx, rx) = oneshot::channel();
    prompt_request_tx.send((request, tx)).await.ok();
    let result = rx.await.expect("Request");
    Ok(warp::reply::json(&serde_json::json!({"request_id": request_id, "result": result})))
}


pub fn routes(idp: Option<Arc<IdentityProvider>>, session: MemoryStore, router: &mut Router) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let prompt_request_tx: mpsc::Sender<PromptChannel> = router.get_address()
        .expect("No prompt request channel defined").clone();

    let test_architect = warp::path("architect")
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_channel(prompt_request_tx.clone()))
        .and_then(test_architect_handler);

    warp::path("prompt").and(test_architect)
}
