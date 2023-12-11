use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
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
        let project = Self { name: name.to_owned(),
                             description: description.to_owned(),
                             n_tasks: 0 };

        diesel::insert_into(crate::schema::projects::table)
            .values(&project)
            .execute(conn)?;
        sender.send(project.clone()).ok();
        Ok(project)
    }
}

impl Project {
    pub fn get(conn: &mut PgConnection, project: &str) -> Option<Self> {
        use crate::schema::projects::dsl;
        let (name, description, n_tasks) = dsl::projects.find(project)
            .get_result::<(String, String, Option<i32>)>(conn)
            .optional()
            .ok()??;
        Some(Project{name, description, n_tasks: n_tasks.unwrap_or(0)})
    }
}
