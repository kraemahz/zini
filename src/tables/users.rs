use diesel::prelude::*;
use serde::Serialize;
use chrono::NaiveDateTime;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::ValidationErrorMessage;
use crate::auth::{generate_salt, salt_and_hash};


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::user_id_accounts)]
pub struct UserIdAccount {
    pub user_id: Uuid,
    pub username: String,
}

#[derive(PartialEq, Clone, Debug, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub created: NaiveDateTime,
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::users)]
pub(crate) struct UserData {
    pub id: Uuid,
    pub email: String,
    pub created: NaiveDateTime,
    pub salt: Option<Vec<u8>>,
    pub hash: Option<Vec<u8>>,
}

impl UserData {
    pub fn from_username(conn: &mut PgConnection, username: &str) -> Option<Self> {
        use crate::schema::users;
        use crate::schema::user_id_accounts;
        let (_account, user): (UserIdAccount, UserData) = user_id_accounts::table
            .inner_join(users::table.on(users::id.eq(user_id_accounts::user_id)))
            .filter(user_id_accounts::username.eq(username))
            .first(conn)
            .optional()
            .ok()??;
        Some(user)
    }

    pub fn from_email(conn: &mut PgConnection, email: &str) -> Option<Self> {
        use crate::schema::users;
        let user: UserData = users::table
            .filter(users::email.eq(email))
            .first(conn)
            .optional()
            .ok()??;
        Some(user)
    }


}

impl From<UserData> for User {
    fn from(data: UserData) -> User {
        User{id: data.id, email: data.email, created: data.created}
    }
}

impl User {
    fn is_valid_username(username: &str) -> bool {
        let first_char_is_alpha = username.chars().next().map_or(false, |c| c.is_alphabetic());
        username.chars().all(|c| c.is_alphanumeric() || c == '_') && 
        username == username.to_lowercase() && 
        !username.contains(' ') && 
        first_char_is_alpha
    }

    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  email: &str,
                  username: Option<&str>,
                  password: Option<&str>) -> QueryResult<Self> {

        if let Some(username) = username {
            if !Self::is_valid_username(username) {
                let kind = diesel::result::DatabaseErrorKind::CheckViolation;
                let msg = Box::new(ValidationErrorMessage{message: "Invalid username".to_string(),
                                                          column: "username".to_string(),
                                                          constraint_name: "username_limits".to_string()});
                return Err(diesel::result::Error::DatabaseError(kind, msg));
            }
        }

        let (salt, hash) = if let Some(password) = password {
            let salt = generate_salt();
            let hash = salt_and_hash(password, &salt);
            let hash = match hash {
                Ok(hash) => hash.as_bytes().to_vec(),
                Err(_) => {
                    let kind = diesel::result::DatabaseErrorKind::CheckViolation;
                    let msg = Box::new(ValidationErrorMessage{message: "Invalid password hash".to_string(),
                                                              column: "hash".to_string(),
                                                              constraint_name: "hashing_algorithm".to_string()});
                    return Err(diesel::result::Error::DatabaseError(kind, msg));
                }
            };
            (Some(salt), Some(hash))
        } else {
            (None, None)
        };

        let user = UserData {
            id: Uuid::new_v4(),
            email: email.to_owned(),
            created: chrono::Utc::now().naive_utc(),
            salt,
            hash,
        };

        diesel::insert_into(crate::schema::users::table)
            .values(&user)
            .execute(conn)?;
        if let Some(username) = username {
            let user_id_account = UserIdAccount {
                user_id: user.id,
                username: username.to_ascii_lowercase()
            };
            diesel::insert_into(crate::schema::user_id_accounts::table)
                .values(&user_id_account)
                .execute(conn)?;
        }

        let user: User = user.into();
        sender.send(user.clone()).ok();
        Ok(user)
    }

    pub fn get(conn: &mut PgConnection, id: Uuid) -> Option<Self> {
        use crate::schema::users::dsl::users;
        users
            .find(id)
            .get_result::<UserData>(conn)
            .optional()
            .ok()?
            .map(|user| user.into())
    }

    pub fn list(conn: &mut PgConnection,
                page: u32,
                page_size: u32) -> Vec<Self> {
        use crate::schema::users::dsl::users;
        let offset = page.saturating_sub(1) * page_size;
        match users
                .limit(page_size as i64)
                .offset(offset as i64)
                .load::<UserData>(conn) {
            Ok(list) => list.into_iter().map(|user| user.into()).collect(),
            Err(err) => {
                tracing::warn!("DB List Query Failed: {:?}", err);
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_user_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name);
        let mut conn = harness.conn(); 
        let (mut tx, _) = broadcast::channel(1);
        let user = User::create(&mut conn, &mut tx, "test@example.com", Some("test_user"), Some("password")).expect("user");
        let user2 = User::get(&mut conn, user.id).expect("user2");
        assert_eq!(user, user2);

        assert!(User::create(&mut conn, &mut tx, "bad_user@example.com", Some("2bad_user"), None).is_err());
        assert!(User::create(&mut conn, &mut tx, "bad_email", Some("bad_user"), None).is_err());
    }
}
