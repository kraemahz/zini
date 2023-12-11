use std::sync::Arc;
use serde::Deserialize;
use tokio::sync::broadcast;
use super::*;
use crate::tables::User;

#[derive(Deserialize)]
pub struct UserPayload {
    username: String, 
    email: String,
}

pub async fn create_user_handler(payload: UserPayload,
                                 db_pool: Arc<DbPool>,
                                 mut sender: broadcast::Sender<User>) -> Result<impl warp::Reply, warp::Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let UserPayload{username, email} = payload;
    match User::create(&mut conn, &mut sender, &username, &email) {
        Ok(user) => user,
        Err(_) => return Err(warp::reject::custom(ConflictError{})),
    };
    Ok(warp::reply::json(&"User created"))
}
