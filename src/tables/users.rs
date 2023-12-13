use diesel::prelude::*;
use serde::Serialize;
use chrono::NaiveDateTime;
use tokio::sync::broadcast;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::users)]
pub struct User {
    pub username: String,
    pub created: NaiveDateTime,
    pub email: String,
}


struct ValidationErrorMessage {
    message: String,
    column: String,
    constraint_name: String
}

impl diesel::result::DatabaseErrorInformation for ValidationErrorMessage {
    fn message(&self) -> &str {
        &self.message
    }
    fn details(&self) -> Option<&str> {
        None
    }
    fn hint(&self) -> Option<&str> {
        None
    }
    fn table_name(&self) -> Option<&str> {
        None
    }
    fn column_name(&self) -> Option<&str> {
        Some(&self.column)
    }
    fn constraint_name(&self) -> Option<&str> {
        Some(&self.constraint_name)
    }
    fn statement_position(&self) -> Option<i32> {
        None
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
                  username: &str,
                  email: &str) -> QueryResult<Self> {
        if !Self::is_valid_username(username) {
            let kind = diesel::result::DatabaseErrorKind::CheckViolation;
            let msg = Box::new(ValidationErrorMessage{message: "Invalid username".to_string(),
                                                      column: "username".to_string(),
                                                      constraint_name: "username_limits".to_string()});
            return Err(diesel::result::Error::DatabaseError(kind, msg));
        }

        let new_user = User {
            username: username.to_owned(),
            created: chrono::Utc::now().naive_utc(),
            email: email.to_owned(),
        };

        diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .execute(conn)?;
        sender.send(new_user.clone()).ok();
        Ok(new_user)
    }

    pub fn get(conn: &mut PgConnection, username: &str) -> Option<Self> {
        use crate::schema::users::dsl;
        let user = dsl::users.find(username)
            .get_result::<User>(conn)
            .optional()
            .ok()??;
        Some(user)
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
        let user = User::create(&mut conn, &mut tx, "test_user", "test@example.com").expect("user");
        let user2 = User::get(&mut conn, "test_user").expect("user2");
        assert_eq!(user, user2);

        assert!(User::create(&mut conn, &mut tx, "2bad_user", "bad_user@example.com").is_err());
        assert!(User::create(&mut conn, &mut tx, "bad_user", "bad_email").is_err());
    }
}
