use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::projects::{ActiveProject, Project};
use subseq_util::tables::{UserTable, ValidationErrorMessage};

subseq_util::create_user_base!();
subseq_util::setup_table_crud!(UserMetadata, crate::schema::auth::metadata::dsl::metadata);
subseq_util::setup_table_crud!(UserPortrait, crate::schema::auth::portraits::dsl::portraits);

impl UserMetadata {
    pub fn create(conn: &mut PgConnection, user_id: Uuid, data: serde_json::Value) -> QueryResult<Self> {
        let meta = Self {
            user_id,
            data
        };
        diesel::insert_into(crate::schema::auth::metadata::table)
            .values(&meta)
            .execute(conn)?;
        Ok(meta)
    }
}

impl UserPortrait {
    pub fn create(conn: &mut PgConnection, user_id: Uuid, portrait: Vec<u8>) -> QueryResult<Self> {
        let portrait = UserPortrait { user_id, portrait };
        diesel::insert_into(crate::schema::auth::portraits::table)
            .values(&portrait)
            .execute(conn)?;
        Ok(portrait)
    }
}

subseq_util::setup_table_crud!(
    UserIdAccount,
    crate::schema::auth::user_id_accounts::dsl::user_id_accounts
);

impl User {
    pub fn get_active_project(&self, conn: &mut PgConnection) -> Option<Project> {
        let active_project = ActiveProject::get(conn, self.id);
        if let Some(active_project) = active_project {
            Project::get(conn, active_project.project_id)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::test::MIGRATIONS;
    use function_name::named;
    use subseq_util::tables::harness::{to_pg_db_name, DbHarness};

    #[test]
    #[named]
    fn test_user_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name, Some(MIGRATIONS));
        let mut conn = harness.conn();
        let user = User::create(
            &mut conn,
            Uuid::new_v4(),
            "test@example.com",
            Some("test_user"),
        )
        .expect("user");
        let user2 = User::get(&mut conn, user.id).expect("user2");
        assert_eq!(user, user2);

        assert!(User::create(
            &mut conn,
            Uuid::new_v4(),
            "bad_user@example.com",
            Some("2bad_user")
        )
        .is_err());
        assert!(User::create(&mut conn, Uuid::new_v4(), "bad_email", Some("bad_user")).is_err());
    }
}
