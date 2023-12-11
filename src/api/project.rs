use std::sync::Arc;
use serde::Deserialize;
use tokio::sync::broadcast;
use warp::{Reply, Rejection};

use super::*;
use crate::session::AuthenticatedUser;
use crate::tables::Project;

#[derive(Deserialize)]
pub struct ProjectPayload {
    name: String, 
    description: Option<String>,
}

pub async fn create_project_handler(payload: ProjectPayload,
                                    _auth: AuthenticatedUser,
                                    db_pool: Arc<DbPool>,
                                    mut sender: broadcast::Sender<Project>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let ProjectPayload{name, description} = payload;
    match Project::create(&mut conn,
                          &mut sender,
                          &name.to_ascii_uppercase(),
                          &description.unwrap_or_else(String::new)) {
        Ok(project) => project,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };
    Ok(warp::reply::json(&"Project created"))
}

pub async fn get_project_handler(project: String,
                                 _auth: AuthenticatedUser,
                                 db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let project = match Project::get(&mut conn, &project) {
        Some(project) => project,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    Ok(warp::reply::json(&project))
}
