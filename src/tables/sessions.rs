use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::sessions)]
pub struct Session {
    pub user_id: Uuid,
    pub token: Vec<u8>
}


impl Session {
    pub fn new(conn: &mut PgConnection, user_id: Uuid, token: Vec<u8>) -> QueryResult<Self> {
        use crate::schema::sessions;
        let session = Session { user_id, token };
        diesel::insert_into(sessions::table)
            .values(&session)
            .execute(conn)?;
        Ok(session)
    }
}
