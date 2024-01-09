use uuid::Uuid;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobResult {
    pub job_id: Uuid,
    pub completion_time: NaiveDateTime,
    pub succeeded: bool,
    pub job_log: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub template_id: Option<Uuid>,
    pub name: String,
    pub container: String,
    pub prompt: String,
}
