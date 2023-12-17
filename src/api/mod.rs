use std::sync::Arc;
use warp::Filter;

pub mod flows;
pub mod users;
pub mod projects;
pub mod sessions;
pub mod tasks;

pub use self::sessions::{
    InvalidSessionToken,
    NoSessionToken,
    SessionStore,
    AuthenticatedUser,
    authenticate
};

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

#[derive(Debug)]
pub struct InvalidConfigurationError {}
impl warp::reject::Reject for InvalidConfigurationError {}

use crate::tables::DbPool;
pub fn with_db(pool: Arc<DbPool>) -> impl Filter<Extract = (Arc<DbPool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || pool.clone())
}
