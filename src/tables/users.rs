use diesel::prelude::*;
use chrono::NaiveDateTime;


#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::users)]
pub struct User {
    pub username: String,
    pub created: NaiveDateTime,
    pub email: String,
}


impl User {
    pub fn create(conn: &mut PgConnection, username: &str, email: &str) -> QueryResult<Self> {
        let new_user = User {
            username: username.to_owned(),
            created: chrono::Utc::now().naive_utc(),
            email: email.to_owned(),
        };

        diesel::insert_into(crate::schema::users::table)
            .values(&new_user)
            .execute(conn)?;
        Ok(new_user)
    }
}
