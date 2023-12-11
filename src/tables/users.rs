use diesel::prelude::*;
use chrono::NaiveDateTime;
use tokio::sync::broadcast;


#[derive(Queryable, Insertable, Clone, Debug)]
#[diesel(table_name = crate::schema::users)]
pub struct User {
    pub username: String,
    pub created: NaiveDateTime,
    pub email: String,
}


impl User {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  username: &str,
                  email: &str) -> QueryResult<Self> {
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
        let (username, created, email) = dsl::users.find(username)
            .get_result::<(String, NaiveDateTime, String)>(conn)
            .optional()
            .ok()??;
        Some(Self{username, created, email})
    }

}
