use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptRequest {
    pub request_id: Uuid,
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
