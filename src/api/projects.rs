use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::{api::*, Router, tables::UserTable};
use subseq_util::oidc::IdentityProvider;
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Reply, Rejection, Filter};
use warp_sessions::{MemoryStore, SessionWithStore};

use crate::tables::{DbPool, Project, Flow, User};

#[derive(Deserialize)]
pub struct ProjectPayload {
    name: String, 
    description: Option<String>,
    default_flow: Option<Uuid>
}

pub async fn create_project_handler(payload: ProjectPayload,
                                    auth: AuthenticatedUser,
                                    session: SessionWithStore<MemoryStore>,
                                    db_pool: Arc<DbPool>,
                                    sender: broadcast::Sender<Project>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
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
        None => Flow::default_flow(&mut conn)
    }.ok_or_else(|| warp::reject::custom(DatabaseError{}))?;

    let project = match Project::create(
            &mut conn,
            &user,
            &name.to_ascii_uppercase(),
            &description.unwrap_or_else(String::new),
            &flow) {
        Ok(project) => project,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };
    sender.send(project.clone()).ok();
    let mut dict = HashMap::new();
    dict.insert("project", project.id.to_string());
    Ok((warp::reply::json(&dict), session))
}

const PAGE_SIZE: u32 = 10;

pub async fn list_projects_handler(page_number: u32,
                                   _auth: AuthenticatedUser,
                                   session: SessionWithStore<MemoryStore>,
                                   db_pool: Arc<DbPool>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let projects = Project::list(&mut conn, page_number, PAGE_SIZE);
    Ok((warp::reply::json(&projects), session))
}


pub async fn get_project_handler(project_id: Uuid,
                                 _auth: AuthenticatedUser,
                                 session: SessionWithStore<MemoryStore>,
                                 db_pool: Arc<DbPool>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let project = match Project::get(&mut conn, project_id) {
        Some(project) => project,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };

    Ok((warp::reply::json(&project), session))
}

#[derive(Deserialize)]
pub struct ProjectSetter {
    project_id: Uuid,
}

pub async fn set_active_project_handler(payload: ProjectSetter,
                                        auth: AuthenticatedUser,
                                        session: SessionWithStore<MemoryStore>,
                                        db_pool: Arc<DbPool>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let ProjectSetter{project_id} = payload;
    let project = match Project::get(&mut conn, project_id) {
        Some(project) => project,
        None => {
            return Err(warp::reject::custom(NotFoundError{}));
        }
    };
    project.set_active_project(&mut conn, auth.id())
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    Ok((warp::reply::json(&project), session))
}

pub async fn get_active_project_handler(auth: AuthenticatedUser,
                                        session: SessionWithStore<MemoryStore>,
                                        db_pool: Arc<DbPool>) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let user = User::get(&mut conn, auth.id())
        .ok_or_else(|| warp::reject::custom(NotFoundError{}))?;
    let project = user.get_active_project(&mut conn);

    Ok((warp::reply::json(&project), session))
}

pub fn routes(idp: Option<Arc<IdentityProvider>>,
              session: MemoryStore,
              pool: Arc<DbPool>,
              router: &mut Router) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let project_tx: broadcast::Sender<Project> = router.announce();

    let create_project = warp::post()
        .and(warp::body::json())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and(with_broadcast(project_tx))
        .and_then(create_project_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let list_projects = warp::path("list")
        .and(warp::get())
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(list_projects_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let set_active_project = warp::path("active")
        .and(warp::body::json())
        .and(warp::put())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(set_active_project_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let get_active_project = warp::path("active")
        .and(warp::get())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_active_project_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let get_project = warp::get()
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_project_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    warp::path("project")
        .and(create_project
             .or(list_projects)
             .or(set_active_project)
             .or(get_active_project)
             .or(get_project))
}
