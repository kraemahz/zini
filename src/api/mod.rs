use std::sync::Arc;
use warp::Filter;

pub mod flows;
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

#[derive(Debug)]
pub struct ParseError {}
impl warp::reject::Reject for ParseError {}

use crate::tables::DbPool;
pub fn with_db(pool: Arc<DbPool>) -> impl Filter<Extract = (Arc<DbPool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}
