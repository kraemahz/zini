use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;
use subseq_util::tables::ValidationErrorMessage;

use super::{Flow, User};

#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub created: NaiveDateTime,
    pub description: String,
    pub n_tasks: i32,
    pub default_flow_id: Uuid
}

impl PartialEq for Project {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id &&
            self.name == other.name &&
            self.owner_id == other.owner_id &&
            self.created.timestamp_micros() == other.created.timestamp_micros() &&
            self.description == other.description &&
            self.n_tasks == other.n_tasks &&
            self.default_flow_id == other.default_flow_id
    }
}

impl Project {
    pub fn create(conn: &mut PgConnection,
                  author: &User,
                  name: &str,
                  description: &str,
                  flow: Option<&Flow>) -> QueryResult<Self> {

        let project = Self {
            id: Uuid::new_v4(),
            name: name.to_ascii_uppercase(),
            owner_id: author.id,
            created: chrono::Utc::now().naive_utc(),
            description: description.to_owned(),
            n_tasks: 0,
            default_flow_id: flow.map(|f| f.id).unwrap_or(Uuid::nil())
        };

        if project.name.len() > 10 {
            let kind = diesel::result::DatabaseErrorKind::CheckViolation;
            let msg = Box::new(ValidationErrorMessage{message: "Invalid project name".to_string(),
                                                      column: "name".to_string(),
                                                      constraint_name: "name_limits".to_string()});
            return Err(diesel::result::Error::DatabaseError(kind, msg));
        }

        diesel::insert_into(crate::schema::projects::table)
            .values(&project)
            .execute(conn)?;
        Ok(project)
    }
}

crate::zini_table!(Project, crate::schema::projects::dsl::projects);


#[cfg(test)]
mod test {
    use super::*;
    use subseq_util::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;
    use crate::tables::test::MIGRATIONS;

    #[test]
    #[named]
    fn test_proj_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name,
                                     Some(MIGRATIONS));
        let mut conn = harness.conn(); 
        let user = User::create(&mut conn, Uuid::new_v4(), "test@example.com", None).expect("user");
        let proj = Project::create(&mut conn,
                                   &user,
                                   "test_proj",
                                   "This is a test",
                                   None).expect("proj");
        let proj2 = Project::get(&mut conn, proj.id).expect("proj2");
        assert_eq!(proj, proj2);
        assert_eq!(proj.name, "TEST_PROJ"); // Forced uppercase
    }
}
