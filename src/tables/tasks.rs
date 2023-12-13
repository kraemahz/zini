use std::collections::HashMap;

use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;

use super::project::Project;
use super::users::User;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
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
        let next_id = format!("{}-{}", project.name.to_uppercase(), project.n_tasks);

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
        let task = dsl::tasks.find(task_id)
            .get_result::<Task>(conn)
            .optional()
            .ok()??;
        Some(task)
    }

    pub fn tags(&self, conn: &mut PgConnection) -> QueryResult<Vec<String>> {
        use crate::schema::task_tags;
        use crate::schema::tags;
        task_tags::table
            .inner_join(tags::table)
            .filter(task_tags::task_id.eq(&self.id))
            .select(tags::name)
            .load::<String>(conn)
    }

    pub fn components(&self, conn: &mut PgConnection) -> QueryResult<Vec<String>> {
        use crate::schema::task_components;
        use crate::schema::components;
        task_components::table
            .inner_join(components::table)
            .filter(task_components::task_id.eq(&self.id))
            .select(components::name)
            .load::<String>(conn)
    }

    pub fn watchers(&self, conn: &mut PgConnection) -> QueryResult<Vec<User>> {
        use crate::schema::task_watchers;
        use crate::schema::users;
        task_watchers::table
            .inner_join(users::table)
            .filter(task_watchers::task_id.eq(&self.id))
            .select(users::all_columns)
            .load::<User>(conn)
    }

    pub fn query(conn: &mut PgConnection,
                 query_dict: &HashMap<String, String>,
                 page: i64,
                 page_size: i64) -> QueryResult<Vec<Self>> {
        use crate::schema::tasks::dsl::*;

        let mut query = tasks.into_boxed();
        for (key, value) in query_dict {
            match key.as_str() {
                "id" => query = query.filter(id.eq(value)),
                "title" => query = query.filter(title.ilike(format!("%{}%", value))),
                "description" => query = query.filter(description.ilike(format!("%{}%", value))),
                "author" => query = query.filter(author.eq(value)),
                "assignee" => query = query.filter(assignee.eq(value)),
                "project" => query = query.filter(project.eq(value)),
                _ => {} // Ignore unknown keys or log them if necessary
            }
        }

        let offset = page.saturating_sub(1) * page_size;

        query
            .limit(page_size)
            .offset(offset)
            .load::<Task>(conn)
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


#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_task_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name);
        let mut conn = harness.conn(); 
        let (mut tx, _) = broadcast::channel(1);
        let (mut user_tx, _) = broadcast::channel(1);
        let (mut proj_tx, _) = broadcast::channel(1);

        let user = User::create(&mut conn, &mut user_tx, "test_user", "test@example.com").expect("user");
        let mut proj = Project::create(&mut conn, &mut proj_tx, "proj", "").expect("proj");
        let task = Task::create(&mut conn, &mut tx, &mut proj, "Task 1", "Do this", &user).expect("task");

        let task2 = Task::get(&mut conn, &task.id).expect("task2");

        assert_eq!(task.id, "PROJ-1");
        assert_eq!(task, task2);
        assert_eq!(proj.n_tasks, 1);
    }
}
