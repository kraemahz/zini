use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::{Filter, Rejection, Reply};

use subseq_util::api::*;
use crate::tables::{DbPool, User};

#[derive(Deserialize)]
pub struct UserPayload {
    username: Option<String>,
    email: String,
}

pub async fn create_user_handler(
    payload: UserPayload,
    db_pool: Arc<DbPool>,
    sender: broadcast::Sender<User>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let UserPayload { username, email } = payload;
    let opt_username = match &username {
        Some(s) => Some(s.as_str()),
        None => None,
    };
    let user = match User::create(&mut conn, Uuid::new_v4(), &email, opt_username) {
        Ok(user) => user,
        Err(_) => return Err(warp::reject::custom(ConflictError {})),
    };
    sender.send(user.clone()).ok();
    Ok(warp::reply::json(&user))
}

pub async fn get_user_handler(
    user_id: Uuid,
    db_pool: Arc<DbPool>
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let user = match User::get(&mut conn, user_id) {
        Some(user) => user,
        None => {
            return Err(warp::reject::custom(NotFoundError{}))
        }
    };
    Ok(warp::reply::json(&user))
}

pub fn routes(
    pool: Arc<DbPool>,
    user_tx: broadcast::Sender<User>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_user = warp::post()
        .and(warp::body::json())
        .and(with_db(pool.clone()))
        .and(with_broadcast(user_tx))
        .and_then(create_user_handler);

    let get_user = warp::get()
        .and(warp::path::param())
        .and(with_db(pool.clone()))
        .and_then(get_user_handler);

    warp::path("user").and(create_user.or(get_user))
}
