use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobResult {
    pub job_id: Uuid,
    pub completion_time: NaiveDateTime,
    pub succeeded: bool,
    pub job_log: String,
}

#[derive(PartialEq, Clone, Debug, Deserialize, Serialize)]
pub enum JobRequestType {
    Help(String)
}


#[derive(PartialEq, Clone, Debug, Serialize)]
pub struct ActionTaken {
    pub action: String,
    pub files_changed: Vec<String>,
}

#[derive(PartialEq, Clone, Debug, Serialize)]
pub enum JobResponseType {
    Help {
        actions_taken: Vec<ActionTaken>,
        result: String,
    },
    Failed
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct JobOwners {
    pub created_id: Uuid,
    pub assignee_id: Uuid,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JobRequest {
    pub request: JobRequestType,
    pub response_beam: String,
    pub job_id: Uuid,
    pub user_id: Uuid,
    pub requested_by_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobResponse {
    pub job_id: Uuid,
    pub response: JobResponseType
}

#[derive(PartialEq, Clone, Debug, Deserialize)]
pub struct DenormalizedJob {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub job_owners: JobOwners,
    pub task_id: Option<Uuid>,
}
