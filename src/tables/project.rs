use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub name: String,
    pub description: String,
    pub n_tasks: i32
}

impl Project {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  name: &str,
                  description: &str) -> QueryResult<Self> {
        let project = Self { name: name.to_uppercase(),
                             description: description.to_owned(),
                             n_tasks: 0 };

        diesel::insert_into(crate::schema::projects::table)
            .values(&project)
            .execute(conn)?;
        sender.send(project.clone()).ok();
        Ok(project)
    }

    pub fn get(conn: &mut PgConnection, project: &str) -> Option<Self> {
        use crate::schema::projects::dsl;
        let (name, description, n_tasks) = dsl::projects.find(project)
            .get_result::<(String, String, Option<i32>)>(conn)
            .optional()
            .ok()??;
        Some(Project{name, description, n_tasks: n_tasks.unwrap_or(0)})
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
        let (mut tx, _) = broadcast::channel(1);

        let proj = Project::create(&mut conn, &mut tx, "test_proj", "This is a test").expect("proj");
        let proj2 = Project::get(&mut conn, "TEST_PROJ").expect("proj2");
        assert_eq!(proj, proj2);
    }
}
