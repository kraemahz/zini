use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diesel::PgConnection;
use serde::Deserialize;
use warp::{Filter, Rejection, Reply};
use uuid::Uuid;

use super::*;
use crate::tables::{Session, users::UserData};
use crate::auth::{generate_random_token, to_base64, salt_and_hash};

pub struct SessionStore {
    sessions: Mutex<HashMap<String, AuthenticatedUser>>,
}

impl SessionStore {
    pub fn new() -> Self {
        SessionStore {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn store_session(&self,
                         conn: &mut PgConnection,
                         token: Vec<u8>,
                         user: AuthenticatedUser) -> String {
        let base64_token = to_base64(&token);
        Session::new(conn, user.id(), token).ok();

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(base64_token.clone(), user);
        base64_token
    }

    pub fn get_user_from_token(&self, token: &str) -> Option<AuthenticatedUser> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(token).cloned()
    }
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser (Uuid);

impl AuthenticatedUser {
    pub fn authenticate(conn: &mut PgConnection, login_info: &LoginInfo) -> Option<Self> {
        let (user, password) = match login_info {
            LoginInfo::Basic{username, password} => {
                (UserData::from_username(conn, username.as_str())?, password)
                
            }
            LoginInfo::Email{email, password} => {
                (UserData::from_email(conn, email.as_str())?, password)
            }
        };

        let salt = &user.salt?;
        let hash = user.hash?;
        let calc_hash = match salt_and_hash(&password, salt) {
            Ok(hash) => hash,
            Err(err) => {
                tracing::warn!("Hashing failed for {} ({}): {:?}", user.email, user.id, err);
                return None;
            }
        };
        let hash = match String::from_utf8(hash) {
            Ok(hash) => hash,
            Err(err) => {
                tracing::warn!("UTF8 conversion failed for {} ({}): {:?}", user.email, user.id, err);
                return None;
            }
        };
        if hash != calc_hash {
            None
        } else {
            Some(Self(user.id))
        }
    }

    pub fn id(&self) -> Uuid {
        self.0
    }
}

#[derive(Debug)]
pub struct InvalidCredentials;
impl warp::reject::Reject for InvalidCredentials{}

#[derive(Debug)]
pub struct InvalidSessionToken;
impl warp::reject::Reject for InvalidSessionToken {}

#[derive(Debug)]
pub struct NoSessionToken;
impl warp::reject::Reject for NoSessionToken {}

#[derive(Deserialize)]
pub enum LoginInfo {
    Basic{username: String, password: String},
    Email{email: String, password: String},
}

pub async fn login_handler(
    login_info: LoginInfo,
    session_store: Arc<SessionStore>,
    db_pool: Arc<DbPool>,
) -> Result<impl Reply, Rejection> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(warp::reject::custom(DatabaseError{})),
    };

    // Authenticate the user
    if let Some(auth_user) = AuthenticatedUser::authenticate(&mut conn, &login_info) {
        // Generate a session token
        let bytes = generate_random_token(256);
        let token = session_store.store_session(
            &mut conn,
            bytes,
            auth_user
        );

        // Create a response and set the session token as a cookie
        let json = warp::reply::json(&"user");
        let response = warp::reply::with_header(
            json,
            "Set-Cookie",
            format!("session-token={}; HttpOnly; Path=/", token)
        );
        Ok(response)
    } else {
        Err(warp::reject::custom(InvalidCredentials))
    }
}

pub fn authenticate(session_store: Arc<SessionStore>) -> 
        impl Filter<Extract = (AuthenticatedUser,), Error = Rejection> + Clone {
    warp::any()
        .and(warp::cookie::optional("session-token"))
        .and_then(move |session_token: Option<String>| {
            let store = session_store.clone();
            async move {
                match session_token {
                    Some(token) => {
                        if let Some(user) = store.get_user_from_token(&token) {
                            Ok(user)
                        } else {
                            tracing::warn!("Invalid session token: {}", token);
                            Err(warp::reject::custom(InvalidSessionToken))
                        }
                    }
                    None => Err(warp::reject::custom(NoSessionToken)),
                }
            }
        })
}

pub fn with_store(store: Arc<SessionStore>) -> impl Filter<Extract = (Arc<SessionStore>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || store.clone())
}

pub fn routes(store: Arc<SessionStore>,
              pool: Arc<DbPool>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let login = warp::put()
        .and(warp::path("login"))
        .and(warp::body::json())
        .and(with_store(store.clone()))
        .and(with_db(pool.clone()))
        .and_then(login_handler);

    warp::path("session")
        .and(login)
}
