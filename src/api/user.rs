use std::sync::Arc;
use serde::Deserialize;
use tokio::sync::broadcast;
use warp::{Filter, Reply, Rejection};

use super::*;
use crate::router::with_broadcast;
use crate::session::SessionStore;
use crate::tables::{DbPool, User};

#[derive(Deserialize)]
pub struct UserPayload {
    username: Option<String>, 
    email: String,
    password: String,
}

pub async fn create_user_handler(payload: UserPayload,
                                 db_pool: Arc<DbPool>,
                                 mut sender: broadcast::Sender<User>) -> Result<impl warp::Reply, warp::Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let UserPayload{username, email, password} = payload;
    let opt_username = match &username { Some(s) => Some(s.as_str()), None => None };
    match User::create(&mut conn, &mut sender, &email, opt_username, Some(&password)) {
        Ok(user) => user,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };
    Ok(warp::reply::json(&"User created"))
}

pub fn user_routes(_store: Arc<SessionStore>,
                   pool: Arc<DbPool>,
                   user_tx: broadcast::Sender<User>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let create_user = warp::post()
        .and(warp::body::json())
        .and(with_db(pool.clone()))
        .and(with_broadcast(user_tx))
        .and_then(create_user_handler);

    warp::path("user")
        .and(create_user)
}
