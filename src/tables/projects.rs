use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::{User, ValidationErrorMessage};

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub created: NaiveDateTime,
    pub description: String,
    pub n_tasks: i32
}

impl Project {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  author: &User,
                  name: &str,
                  description: &str) -> QueryResult<Self> {
        let project = Self {
            id: Uuid::new_v4(),
            name: name.to_ascii_uppercase(),
            owner_id: author.id,
            created: chrono::Utc::now().naive_utc(),
            description: description.to_owned(),
            n_tasks: 0
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
        sender.send(project.clone()).ok();
        Ok(project)
    }

    pub fn get(conn: &mut PgConnection, project_id: Uuid) -> Option<Self> {
        use crate::schema::projects::dsl;
        let (id, name, owner_id, created, description, n_tasks) = dsl::projects.find(project_id)
            .get_result::<(Uuid, String, Uuid, NaiveDateTime, String, Option<i32>)>(conn)
            .optional()
            .ok()??;
        Some(Project{id, name, owner_id, created, description, n_tasks: n_tasks.unwrap_or(0)})
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_proj_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name);
        let mut conn = harness.conn(); 
        let (mut user_tx, _) = broadcast::channel(1);
        let (mut tx, _) = broadcast::channel(1);

        let user = User::create(&mut conn, &mut user_tx, "test@example.com", None, None).expect("user");
        let proj = Project::create(&mut conn, &mut tx, &user, "test_proj", "This is a test").expect("proj");
        let proj2 = Project::get(&mut conn, proj.id).expect("proj2");
        assert_eq!(proj, proj2);
        assert_eq!(proj.name, "TEST_PROJ"); // Forced uppercase
    }
}
