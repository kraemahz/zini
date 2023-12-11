use std::sync::Arc;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::prelude::*;
use warp::Filter;

pub mod user;
pub mod project;
pub mod tasks;

#[derive(Debug)]
pub struct ConflictError {}
impl warp::reject::Reject for ConflictError {}

#[derive(Debug)]
pub struct DatabaseError {}
impl warp::reject::Reject for DatabaseError {}

#[derive(Debug)]
pub struct NotFoundError {}
impl warp::reject::Reject for NotFoundError {}

pub type DbPool = Pool<ConnectionManager<PgConnection>>;

pub fn with_db(pool: Arc<DbPool>) -> impl Filter<Extract = (Arc<DbPool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}
