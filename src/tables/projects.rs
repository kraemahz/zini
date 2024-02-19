use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use subseq_util::tables::ValidationErrorMessage;
use uuid::Uuid;

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
    pub default_flow_id: Uuid,
}

impl PartialEq for Project {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.owner_id == other.owner_id
            && self.created.timestamp_micros() == other.created.timestamp_micros()
            && self.description == other.description
            && self.n_tasks == other.n_tasks
            && self.default_flow_id == other.default_flow_id
    }
}

impl Project {
    pub fn create(
        conn: &mut PgConnection,
        author: &User,
        name: &str,
        description: &str,
        flow: &Flow,
    ) -> QueryResult<Self> {
        let project = Self {
            id: Uuid::new_v4(),
            name: name.to_ascii_uppercase(),
            owner_id: author.id,
            created: chrono::Utc::now().naive_utc(),
            description: description.to_owned(),
            n_tasks: 0,
            default_flow_id: flow.id,
        };

        if project.name.len() > 64 {
            let kind = diesel::result::DatabaseErrorKind::CheckViolation;
            let msg = Box::new(ValidationErrorMessage {
                message: "Invalid project name".to_string(),
                column: "name".to_string(),
                constraint_name: "name_limits".to_string(),
            });
            return Err(diesel::result::Error::DatabaseError(kind, msg));
        }

        diesel::insert_into(crate::schema::projects::table)
            .values(&project)
            .execute(conn)?;
        Ok(project)
    }

    pub fn set_active_project(&self, conn: &mut PgConnection, uid: Uuid) -> QueryResult<()> {
        use crate::schema::active_projects::dsl::*;
        let pid = self.id;

        diesel::insert_into(active_projects)
            .values(&ActiveProject {
                user_id: uid,
                project_id: pid,
            })
            .on_conflict(user_id)
            .do_update()
            .set(project_id.eq(pid))
            .execute(conn)?;
        Ok(())
    }
}

subseq_util::setup_table_crud!(Project, crate::schema::projects::dsl::projects);

#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::active_projects)]
pub struct ActiveProject {
    pub user_id: Uuid,
    pub project_id: Uuid,
}

subseq_util::setup_table_crud!(
    ActiveProject,
    crate::schema::active_projects::dsl::active_projects
);

#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::test::MIGRATIONS;
    use crate::tables::Flow;
    use function_name::named;
    use subseq_util::tables::harness::{to_pg_db_name, DbHarness};
    use subseq_util::tables::UserTable;

    #[test]
    #[named]
    fn test_proj_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name, Some(MIGRATIONS));
        let mut conn = harness.conn();
        let user = User::create(&mut conn, Uuid::new_v4(), "test@example.com", None).expect("user");
        let flow = Flow::default_flow(&mut conn).expect("default flow");
        let proj =
            Project::create(&mut conn, &user, "test_proj", "This is a test", &flow).expect("proj");
        let proj2 = Project::get(&mut conn, proj.id).expect("proj2");
        assert_eq!(proj, proj2);
        assert_eq!(proj.name, "TEST_PROJ"); // Forced uppercase
    }
}
