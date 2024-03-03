use std::sync::Arc;

use bytes::Buf;
use chrono::NaiveDateTime;
use diesel::{PgConnection, QueryResult};
use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use subseq_util::api::InvalidConfigurationError;
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::{
    api::{authenticate, with_db, AuthenticatedUser, DatabaseError},
    oidc::IdentityProvider,
    tables::{DbPool, UserTable},
};
use uuid::Uuid;
use warp::multipart::FormData;
use warp::{http::Response, reject::Rejection, reply::Reply, Filter};
use warp_sessions::{MemoryStore, SessionWithStore};

use crate::tables::{User, UserIdAccount, UserMetadata, UserPortrait};
use super::PAGE_SIZE;

#[derive(Deserialize, Serialize)]
pub struct StoredUserMeta {
    pub job_title: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct DenormalizedUser {
    pub id: Uuid,
    pub created: NaiveDateTime,
    pub username: String,
    pub job_title: String,
    pub job_id: Option<String>,
}

impl DenormalizedUser {
    const TITLE_MISSING: &'static str = "Title Not Set";

    pub fn denormalize(conn: &mut PgConnection, user: User) -> QueryResult<Self> {
        let username = match UserIdAccount::get(conn, user.id) {
            Some(uida) => uida.username,
            None => user.email.clone(),
        };
        let job_title = match UserMetadata::get(conn, user.id) {
            Some(umeta) => match serde_json::from_value::<StoredUserMeta>(umeta.data) {
                Ok(sumeta) => sumeta.job_title,
                Err(_) => Self::TITLE_MISSING.to_string(),
            },
            None => Self::TITLE_MISSING.to_string(),
        };
        Ok(Self {
            id: user.id,
            created: user.created,
            username,
            job_title,
            job_id: None,
        })
    }
}

async fn get_image(
    user_id: Uuid,
    _auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let portrait = match UserPortrait::get(&mut conn, user_id) {
        Some(portrait) => portrait,
        None => return Err(warp::reject::not_found()),
    };
    Ok((
        Response::builder()
            .header("Content-Type", "image/webp")
            .body(portrait.portrait)
            .unwrap(),
        session,
    ))
}

pub async fn list_users_handler(
    page: u32,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let users = User::list(&mut conn, page, PAGE_SIZE);
    let mut denorm_users = Vec::new();
    for user in users {
        let denorm_user = DenormalizedUser::denormalize(&mut conn, user)
            .map_err(|_| warp::reject::custom(DatabaseError {}))?;
        denorm_users.push(denorm_user);
    }
    Ok((warp::reply::json(&denorm_users), session))
}

pub async fn upload_portrait_handler(
    user_id: Uuid,
    form_data: FormData,
    _auth: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>,
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut parts = form_data.into_stream();
    let mut file_data = vec![];

    while let Some(Ok(part)) = parts.next().await {
        let data = part
            .stream()
            .try_fold(Vec::new(), |mut acc, buf| async move {
                acc.extend_from_slice(buf.chunk());
                Ok(acc)
            })
            .await
            .map_err(|_| warp::reject::custom(DatabaseError {}))?;
        file_data.extend_from_slice(&data);
    }

    if file_data.is_empty() {
        return Err(warp::reject::custom(InvalidConfigurationError {}));
    }
    tracing::info!("Portrait with {} bytes", file_data.len());
    let mut conn = db_pool
        .get()
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;

    let portrait = UserPortrait::create(&mut conn, user_id, file_data)
        .map_err(|_| warp::reject::custom(DatabaseError {}))?;
    Ok((warp::reply::json(&portrait), session))
}

pub fn routes(
    idp: Option<Arc<IdentityProvider>>,
    session: MemoryStore,
    pool: Arc<DbPool>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let list_users = warp::path("user")
        .and(warp::path("list"))
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(list_users_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    let get_image = warp::path("portrait")
        .and(warp::get())
        .and(warp::path::param())
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(get_image)
        .untuple_one()
        .and_then(store_auth_cookie);

    let put_image = warp::path("portrait")
        .and(warp::path::param())
        .and(warp::multipart::form().max_length(10 * 1024 * 1024))
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(upload_portrait_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    list_users.or(get_image).or(put_image)
}
