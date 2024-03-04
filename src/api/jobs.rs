use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;
use serde::{Serialize, Deserialize};
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::oidc::IdentityProvider;
use subseq_util::{api::*, tables::{DbPool, UserTable}, Router};
use tokio::spawn;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;
use warp::{Filter, Rejection, Reply};
use warp_sessions::{MemoryStore, SessionWithStore};

use crate::interop::{JobResult, ActionTaken};
use crate::tables::{HelpResolution, DenormalizedHelpAction};
use crate::{
    interop::{
        DenormalizedJob,
        JobRequestType,
        JobRequest,
        JobResponseType,
        JobResponse
    },
    tables::{
        User,
        Job,
        JobResult as JobResultTable,
        AwaitingHelp
    }
};

use super::prompts::{PromptRxPayload, PromptChannelHandle};

pub fn handle_new_job(db_pool: Arc<DbPool>, router: &mut Router) {
    let mut job_rx: mpsc::Receiver<DenormalizedJob> = router.create_channel();

    spawn(async move {
        while let Some(job) = job_rx.recv().await {
            // Create a new job object from this incoming job
            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => {
                    tracing::warn!("Database connection failed");
                    continue;
                }
            };

            let DenormalizedJob{
                id,
                project_id,
                task_id,
                name,
                job_owners
            } = job;

            let task_id = match task_id {
                Some(t) => t,
                None => {
                    tracing::warn!("Missing task id from job {}", id);
                    continue;
                }
            };

            match Job::create(
                &mut conn,
                id,
                project_id,
                task_id,
                name,
                job_owners.created_id,
                job_owners.assignee_id,
            ) {
                Ok(job) => tracing::info!("Job created {}", job.id),
                Err(err) => tracing::info!("Failed Job::create: {:?}", err),
            };
        }
        tracing::warn!("handle_new_job exited");
    });
}

pub fn handle_new_job_results(db_pool: Arc<DbPool>, router: &mut Router) {
    let mut job_result_rx: mpsc::Receiver<JobResult> = router.create_channel();

    spawn(async move {
        while let Some(job_result) = job_result_rx.recv().await {
            let JobResult {
                job_id,
                completion_time,
                succeeded,
                job_log
            } = job_result;
            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => {
                    tracing::warn!("Database connection failed");
                    continue;
                }
            };
            // Find the associated job
            let (job, result) = match Job::get(&mut conn, job_id) {
                Some(t) => t,
                None => {
                    tracing::warn!("No matching job {}", job_id);
                    continue;
                }
            };

            if result.is_some() {
                tracing::warn!("Job already has result {}", job_id);
                continue;
            }

            // Insert this result with that job
            match JobResultTable::create(&mut conn,
                                   &job,
                                   completion_time,
                                   succeeded,
                                   job_log) {
                Ok(job) => tracing::info!("JobResult created {}", job.job_id),
                Err(err) => tracing::info!("Failed JobResult::create: {:?}", err),
            }
        }
        tracing::warn!("handle_new_job_results exited");
    });
}

pub fn handle_job_request(db_pool: Arc<DbPool>, router: &mut Router, prompt_channel: PromptChannelHandle) {
    let mut job_request_rx: mpsc::Receiver<JobRequest> = router.create_channel();
    let job_response_tx: broadcast::Sender<JobResponse> = router.announce();

    spawn(async move {
        while let Some(request) = job_request_rx.recv().await {
            let JobRequest{job_id, response_beam: _, request, user_id, requested_by_id} = request;

            let failed = JobResponse {
                job_id,
                response: JobResponseType::Failed
            };

            let mut conn = match db_pool.get() {
                Ok(conn) => conn,
                Err(_) => {
                    tracing::warn!("Database connection failed");
                    job_response_tx.send(failed).ok();
                    continue;
                }
            };

            // Fetch the assigned user
            let assigned_user = match User::get(&mut conn, user_id) {
                Some(user) => user,
                None => {
                    tracing::warn!("No such user: {}", user_id);
                    job_response_tx.send(failed).ok();
                    continue;
                }
            };

            match prompt_channel.get_user_tx(requested_by_id) {
                Some(tx) => {
                    // Send this request to the instructions UI
                    let prompt_payload = PromptRxPayload::JobRequest(
                        request.clone(),
                        assigned_user,
                    );
                    if tx.send(prompt_payload).is_err() {
                        tracing::warn!("Channel for user {} is closed", requested_by_id);
                    }
                },
                None => {
                    tracing::warn!("No open channel for user {}", requested_by_id);
                }
            };

            // Find the the active job
            let (job, _) = match Job::get(&mut conn, job_id) {
                Some(j) => j,
                None => {
                    tracing::warn!("No matching job {}", job_id);
                    job_response_tx.send(failed).ok();
                    continue;
                }
            };
            let request = match request {
                JobRequestType::Help(request) => request,
            };

            // Add this request to the DB
            match AwaitingHelp::create(&mut conn,
                                        job.id,
                                        request.clone()) {
                Ok(request) => request,
                Err(_) => {
                    tracing::warn!("Couldnt't create request {}", job_id);
                    job_response_tx.send(failed).ok();
                    continue;
                }
            };

        }
        tracing::warn!("handle_job_requests exited");
    });
}

type JobIdToBeam = HashMap<Uuid, String>;

#[derive(Clone, Debug)]
pub struct JobRequestCollection {
    inner: Arc<Mutex<JobIdToBeam>>,
    tx: mpsc::Sender<JobRequest>,
}

impl JobRequestCollection {
    pub fn new(tx: mpsc::Sender<JobRequest>) -> Self {
        Self { inner: Arc::new(Mutex::new(JobIdToBeam::new())), tx }
    }

    pub async fn insert(&self, request: JobRequest) {
        {
            let mut map = self.inner.lock().unwrap();
            map.insert(request.job_id, request.response_beam.clone());
        }
        self.tx.send(request).await.ok();
    }

    pub fn remove(&self, job_id: Uuid) -> Option<String> {
        let mut map = self.inner.lock().unwrap();
        map.remove(&job_id)
    }
}

#[derive(Deserialize)]
pub struct QueryPayload {
    pub page: u32,
    pub page_size: u32,
    pub query: HashMap<String, String>,
}

async fn filter_jobs_handler(
    payload:  QueryPayload,
    auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let QueryPayload { page, page_size, query } = payload;

    let result = Job::query(&mut conn, auth_user.id(), &query, page, page_size)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;

    let jobs: Vec<_> = result.into_iter()
        .map(|(job, job_result)| {
            DenormalizedJobPartial {
                id: job.id,
                project_id: job.project_id,
                job_name: job.name,
                succeeded: job_result.map(|r| r.succeeded)
            }
        })
        .collect();

    Ok((warp::reply::json(&jobs), session))
}

async fn filter_logs_handler(
    payload:  QueryPayload,
    auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };

    let QueryPayload { page, page_size, mut query } = payload;
    query.insert("running".to_string(), "false".to_string());  // Guarantees all have results

    let result = Job::query(&mut conn, auth_user.id(), &query, page, page_size)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;

    let jobs: Vec<_> = result.into_iter()
        .map(|(job, job_result)| {
            let job_result = job_result.expect("all jobs are completed");
            DenormalizedJobLogs {
                id: job.id,
                project_id: job.project_id,
                job_name: job.name,
                completion_time: job_result.completion_time,
                succeeded: job_result.succeeded,
                job_log: job_result.job_log,
            }
        })
        .collect();

    Ok((warp::reply::json(&jobs), session))
}

#[derive(Serialize)]
pub struct DenormalizedHelpRequest {
    pub initial_request: String,
    pub help_resolution: Option<HelpResolution>,
    pub help_actions: Vec<DenormalizedHelpAction>,
}

#[derive(Serialize)]
pub struct DenormalizedJobResult {
    pub completion_time: NaiveDateTime,
    pub succeeded: bool,
    pub job_log: String
}

#[derive(Serialize)]
pub struct DenormalizedJobPartial {
    pub id: Uuid,
    pub project_id: Uuid,
    pub job_name: String,
    pub succeeded: Option<bool>,
}

#[derive(Serialize)]
pub struct DenormalizedJobLogs {
    pub id: Uuid,
    pub project_id: Uuid,
    pub job_name: String,
    pub completion_time: NaiveDateTime,
    pub succeeded: bool,
    pub job_log: String
}

#[derive(Serialize)]
pub struct DenormalizedJobFull {
    pub id: Uuid,
    pub project_id: Uuid,
    pub job_name: String,
    pub result: Option<DenormalizedJobResult>,
    pub help_request: Option<DenormalizedHelpRequest>,
}

async fn get_job_handler(
    job_id: Uuid,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };

    let (job, job_result) = match Job::get(&mut conn, job_id) {
        Some(job) => job,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    let help = AwaitingHelp::next_open_help(&mut conn, &job);
    let help_request = if let Some(help) = help.as_ref() {
        let resolution = HelpResolution::get(&mut conn, help.id);
        let help_actions = match DenormalizedHelpAction::list(&mut conn, help.id) {
            Ok(actions) => actions,
            Err(_) => vec![]
        };
        Some(DenormalizedHelpRequest {
            initial_request: help.request.clone(),
            help_resolution: resolution,
            help_actions,
        })
    } else {
        None
    };

    let job_result = job_result.map(|result|
        DenormalizedJobResult {
            completion_time: result.completion_time,
            succeeded: result.succeeded,
            job_log: result.job_log
        });

    let denormalized_job = DenormalizedJobFull {
        id: job.id,
        project_id: job.project_id,
        job_name: job.name,
        result: job_result,
        help_request
    };

    Ok((warp::reply::json(&denormalized_job), session))
}

#[derive(Deserialize)]
struct HelpStep {
    action_taken: String,
    files_changed: Vec<String>
}

async fn create_help_step_handler(
    job_id: Uuid,
    help_step: HelpStep,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    // Find job
    let (job, _) = match Job::get(&mut conn, job_id) {
        Some(job) => job,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    let help = match AwaitingHelp::next_open_help(&mut conn, &job) {
        Some(help) => help,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    let action = DenormalizedHelpAction::create(&mut conn,
                                                &help,
                                                help_step.action_taken,
                                                help_step.files_changed)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    Ok((warp::reply::json(&action), session))
}

async fn update_help_step_handler(
    _job_id: Uuid,
    step_id: Uuid,
    help_step: HelpStep,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let mut help_action = match DenormalizedHelpAction::get(&mut conn, step_id) {
        Some(help_action) => help_action,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    help_action.update(&mut conn, help_step.action_taken, help_step.files_changed)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    Ok((warp::reply::json(&help_action), session))
}

async fn delete_help_step_handler(
    _job_id: Uuid,
    step_id: Uuid,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };

    let help = match DenormalizedHelpAction::get(&mut conn, step_id) {
        Some(help) => help,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    help.delete(&mut conn)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    Ok((warp::reply::reply(), session))
}

#[derive(Deserialize)]
struct FinishHelp {
    result: String
}

async fn finish_help_handler(
    job_id: Uuid,
    finish_help: FinishHelp,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
    job_response_tx: broadcast::Sender<JobResponse>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    // Find job
    let (job, _) = match Job::get(&mut conn, job_id) {
        Some(job) => job,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    let help = match AwaitingHelp::next_open_help(&mut conn, &job) {
        Some(help) => help,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    // Find help task
    let help_resolution = HelpResolution::create(&mut conn, &help, finish_help.result)
        .map_err(|err| {
            tracing::error!("Help resolution failed: {:?}", err);
            warp::reject::custom(ConflictError {})
        })?;

    let actions = DenormalizedHelpAction::list(&mut conn, help.id)
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;

    let mut actions_taken = vec![];
    for action in actions {
        let action = ActionTaken {
            action: action.action_taken,
            files_changed: action.files_changed
        };
        actions_taken.push(action);
    }

    let job_response = JobResponse {
        job_id,
        response: JobResponseType::Help {
            actions_taken,
            result: help_resolution.result.clone()
        }
    };
    job_response_tx.send(job_response).ok();

    Ok((warp::reply::json(&help_resolution), session))
}

pub fn routes(
    idp: Option<Arc<IdentityProvider>>,
    session: MemoryStore,
    pool: Arc<DbPool>,
    router: &mut Router,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let job_response_tx: broadcast::Sender<JobResponse> = router.announce();

    let filter_jobs = warp::path("query")
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(filter_jobs_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let filter_logs = warp::path!("log" / "query")
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(filter_logs_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let get_job = warp::get()
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_job_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let create_help_step = warp::path::param()
        .and(warp::path("action"))
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(create_help_step_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let update_help_step = warp::path::param()
        .and(warp::path!("action" / Uuid))
        .and(warp::put())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(update_help_step_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let delete_help_step = warp::path::param()
        .and(warp::path!("action" / Uuid))
        .and(warp::delete())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(delete_help_step_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let finish_help = warp::path::param()
        .and(warp::path("finish"))
        .and(warp::post())
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(job_response_tx))
        .and_then(finish_help_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    warp::path("job").and(
        filter_jobs
            .or(filter_logs)
            .or(get_job)
            .or(create_help_step)
            .or(update_help_step)
            .or(delete_help_step)
            .or(finish_help)
    )
}
