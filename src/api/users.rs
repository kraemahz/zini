use std::sync::Arc;

use bytes::{BytesMut, BufMut};
use diesel::{PgConnection, QueryResult};
use chrono::NaiveDateTime;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use subseq_util::api::InvalidConfigurationError;
use subseq_util::api::sessions::store_auth_cookie;
use subseq_util::{
    api::{AuthenticatedUser, DatabaseError, authenticate, with_db},
    oidc::IdentityProvider,
    tables::{DbPool, UserTable}
};
use uuid::Uuid;
use warp::{Filter, http::Response, reply::Reply, reject::Rejection};
use warp_sessions::{MemoryStore, SessionWithStore};
use warp::multipart::{FormData, Part};
use crate::tables::{UserIdAccount, User, UserMetadata, UserPortrait};

#[derive(Deserialize, Serialize)]
pub struct StoredUserMeta {
    job_title: String
}

#[derive(Serialize)]
pub struct DenormalizedUser {
    pub id: Uuid,
    pub created: NaiveDateTime,
    pub username: String,
    pub job_title: String,
    pub job_id: Option<String>
}

impl DenormalizedUser {
    const TITLE_MISSING: &'static str = "Title Not Set";

    pub fn denormalize(conn: &mut PgConnection, user: User) -> QueryResult<Self> {
        let username = match UserIdAccount::get(conn, user.id) {
            Some(uida) => uida.username,
            None => user.email.clone()
        };
        let job_title = match UserMetadata::get(conn, user.id) {
            Some(umeta) => match serde_json::from_value::<StoredUserMeta>(umeta.data) {
                Ok(sumeta) => sumeta.job_title,
                Err(_) => Self::TITLE_MISSING.to_string()
            }
            None => Self::TITLE_MISSING.to_string()
        };
        Ok(Self {
            id: user.id,
            created: user.created,
            username,
            job_title,
            job_id: None
        })
    }
}

async fn get_image(user_id: Uuid,
                   _auth: AuthenticatedUser,
                   session: SessionWithStore<MemoryStore>,
                   db_pool: Arc<DbPool>
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };
    let portrait = match UserPortrait::get(&mut conn, user_id) {
        Some(portrait) => portrait,
        None => return Err(warp::reject::not_found())
    };
    Ok((Response::builder()
       .header("Content-Type", "image/webp")
       .body(portrait.portrait)
       .unwrap(), session))
}

const USER_PAGE_SIZE: u32 = 20;

pub async fn list_users_handler(
    page: u32,
    _auth_user: AuthenticatedUser,
    session: SessionWithStore<MemoryStore>,
    db_pool: Arc<DbPool>
) -> Result<(impl Reply, SessionWithStore<MemoryStore>), Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError {})),
    };
    let users = User::list(&mut conn, page, USER_PAGE_SIZE);
    let mut denorm_users = Vec::new();
    for user in users {
        let denorm_user = DenormalizedUser::denormalize(&mut conn, user)
            .map_err(|_| warp::reject::custom(DatabaseError{}))?;
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
    let portrait_bytes = process_multipart(form_data).await
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    let mut conn = db_pool.get().map_err(|_| warp::reject::custom(DatabaseError{}))?;
    let portrait = UserPortrait::create(&mut conn, user_id, portrait_bytes)
        .map_err(|_| warp::reject::custom(DatabaseError{}))?;
    Ok((warp::reply::json(&portrait), session))
}

async fn process_multipart(form_data: FormData) -> Result<Vec<u8>, warp::Rejection> {
    let parts: Vec<Part> = form_data.try_collect().await.map_err(|e| {
        eprintln!("Error collecting form data: {}", e);
        warp::reject::custom(InvalidConfigurationError{})
    })?;
    let mut file_bytes = BytesMut::new();
    for part in parts {
        // You can use part.name() to get the field name and handle different fields differently
        // Here, we assume there's a file part without checking its name or filename.
        // For a more robust implementation, check part.name() and part.filename() as needed.
        if let Some(_filename) = part.filename() {
            let value = part.stream().try_fold(BytesMut::new(), |mut acc, bytes| async move {
                acc.put(bytes);
                Ok(acc)
            }).await.map_err(|e| {
                eprintln!("Error processing file part: {}", e);
                warp::reject::custom(InvalidConfigurationError{})
            })?;
            
            file_bytes = value;
        }
    }

    Ok(file_bytes.to_vec())
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
        .and(warp::put())
        .and(warp::path::param())
        .and(warp::multipart::form().max_length(1024 * 1024))
        .and(authenticate(idp.clone(), session.clone()))
        .and(with_db(pool.clone()))
        .and_then(upload_portrait_handler)
        .untuple_one()
        .and_then(store_auth_cookie);

    list_users.or(get_image).or(put_image)
}
