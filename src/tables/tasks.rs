use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;

use super::project::Project;
use super::users::User;


#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::tasks)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub assignee: Option<String>,
    pub project: Option<String>,
}

impl Task {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  project: &mut Project,
                  title: &str,
                  description: &str,
                  author: &User) -> QueryResult<Self> {

        project.n_tasks += 1;
        let next_id = format!("{}-{}", project.name, project.n_tasks);

        let task = Self {
            id: next_id,
            title: title.to_owned(),
            description: description.to_owned(),
            author: author.username.clone(),
            assignee: None,
            project: Some(project.name.clone()),
        };
        conn.transaction(|transact| {
            diesel::insert_into(crate::schema::tasks::table)
                .values(&task)
                .execute(transact)?;
            diesel::update(crate::schema::projects::table.find(&project.name))
                .set(crate::schema::projects::n_tasks.eq(project.n_tasks))
                .execute(transact)?;
            QueryResult::Ok(())
        })?;
        sender.send(task.clone()).ok();

        Ok(task)
    }

    pub fn get(conn: &mut PgConnection, task_id: &str) -> Option<Self> {
        use crate::schema::tasks::dsl;
        let (id, title, description, author, assignee, project) = dsl::tasks.find(task_id)
            .get_result::<(String, String, String, String, Option<String>, Option<String>)>(conn)
            .optional()
            .ok()??;
        Some(Task{id, title, description, author, assignee, project})
    }

    pub fn tags(&self) -> Vec<String> {
        vec![] // TODO
    }

    pub fn components(&self) -> Vec<String> {
        vec![] // TODO
    }

    pub fn watchers(&self) -> Vec<User> {
        vec![] // TODO
    }
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::tags)]
struct Tag {
    pub name: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::components)]
struct Component {
    pub name: String,
}

// Join tables for many-to-many relationships
#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_tags)]
struct TaskTag {
    pub task_id: String,
    pub tag_name: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_components)]
struct TaskComponent {
    pub task_id: String,
    pub component_name: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_watchers)]
struct TaskWatcher {
    pub task_id: String,
    pub watcher_username: String,
}
