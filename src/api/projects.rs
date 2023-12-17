use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::broadcast;
use warp::{Reply, Rejection};
use uuid::Uuid;

use super::*;
use crate::router::with_broadcast;
use crate::tables::{DbPool, Project, User, Flow};

#[derive(Deserialize)]
pub struct ProjectPayload {
    name: String, 
    description: Option<String>,
    default_flow: Option<Uuid>
}

pub async fn create_project_handler(payload: ProjectPayload,
                                    auth: AuthenticatedUser,
                                    db_pool: Arc<DbPool>,
                                    mut sender: broadcast::Sender<Project>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let ProjectPayload{name, description, default_flow} = payload;

    let user = match User::get(&mut conn, auth.id()) {
        Some(user) => user,
        None => return Err(warp::reject::custom(NotFoundError{})),
    };
    let flow = match default_flow {
        Some(flow_id) => Flow::get(&mut conn, flow_id),
        None => None
    };

    let project = match Project::create(
            &mut conn,
            &mut sender,
            &user,
            &name.to_ascii_uppercase(),
            &description.unwrap_or_else(String::new),
            flow.as_ref()) {
        Ok(project) => project,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };
    let mut dict = HashMap::new();
    dict.insert("project", project.id.to_string());
    Ok(warp::reply::json(&dict))
}

const PAGE_SIZE: u32 = 10;

pub async fn list_projects_handler(page_number: u32,
                                   _auth: AuthenticatedUser,
                                   db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let projects = Project::list(&mut conn, page_number, PAGE_SIZE);
    Ok(warp::reply::json(&projects))
}


pub async fn get_project_handler(project_id: String,
                                 _auth: AuthenticatedUser,
                                 db_pool: Arc<DbPool>) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let project_id = match Uuid::parse_str(&project_id) {
        Ok(project_id) => project_id,
        Err(_) => {
            return Err(warp::reject::custom(ParseError{}));
        }
    };
    let project = match Project::get(&mut conn, project_id) {
        Some(project) => project,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    Ok(warp::reply::json(&project))
}

pub fn routes(store: Arc<SessionStore>,
              pool: Arc<DbPool>,
              project_tx: broadcast::Sender<Project>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_project = warp::post()
        .and(warp::body::json())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(project_tx))
        .and_then(create_project_handler);

    let list_projects = warp::path("list")
        .and(warp::get())
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(list_projects_handler);

    let get_project = warp::get()
        .and(warp::path::param())
        .and(authenticate(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_project_handler);

    warp::path("project")
        .and(create_project
             .or(list_projects)
             .or(get_project))
}
