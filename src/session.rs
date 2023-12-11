use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use warp::{Filter, Rejection, Reply};

pub struct SessionStore {
    sessions: Mutex<HashMap<String, AuthenticatedUser>>,
}

impl SessionStore {
    pub fn new() -> Self {
        SessionStore {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn store_session(&self, token: &str, user: AuthenticatedUser) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(token.to_string(), user);
    }

    pub fn get_user_from_token(&self, token: &str) -> Option<AuthenticatedUser> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(token).cloned()
    }
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub username: String,
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
pub struct LoginInfo {
}


pub async fn login_handler(
    login_info: LoginInfo,
    session_store: Arc<SessionStore>
) -> Result<impl Reply, Rejection> {
    // Authenticate the user
    if let Some(user) = authenticate_user(&login_info).await {
        // Generate a session token
        let token = generate_session_token();

        // Store the session token
        session_store.store_session(&token, AuthenticatedUser{username: user});

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

pub fn generate_session_token() -> String {
    // TODO
    "A very secure token".to_string()
}

pub async fn authenticate_user(login_info: &LoginInfo) -> Option<String> {
    // TODO
    Some("default".to_string())
}

pub fn authenticate(session_store: Arc<SessionStore>) -> 
        impl Filter<Extract = (AuthenticatedUser,), Error = Rejection> + Clone {
    warp::any()
        .and(warp::header::optional("session-token"))
        .and_then(move |session_token: Option<String>| {
            let store = session_store.clone();
            async move {
                match session_token {
                    Some(token) => {
                        if let Some(user) = store.get_user_from_token(&token) {
                            Ok(user)
                        } else {
                            Err(warp::reject::custom(InvalidSessionToken))
                        }
                    }
                    None => Err(warp::reject::custom(NoSessionToken)),
                }
            }
        })
}
