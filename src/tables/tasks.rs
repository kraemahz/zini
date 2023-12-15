use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::projects::Project;
use super::users::User;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::task_projects)]
pub struct TaskProject {
    pub task_id: Uuid,
    pub project_id: Uuid
}


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::tasks)]
pub struct Task {
    pub id: Uuid,
    pub slug: String,
    pub created: NaiveDateTime,
    pub title: String,
    pub description: String,
    pub author_id: Uuid,
    pub assignee_id: Option<Uuid>,
}

impl Task {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  project: &mut Project,
                  title: &str,
                  description: &str,
                  author: &User) -> QueryResult<Self> {

        project.n_tasks += 1;
        let slug = format!("{}-{}", project.name, project.n_tasks);

        let task = Self {
            id: Uuid::new_v4(),
            slug,
            created: chrono::Utc::now().naive_utc(),
            title: title.to_owned(),
            description: description.to_owned(),
            author_id: author.id.clone(),
            assignee_id: None,
        };

        let task_project = TaskProject {
            task_id: task.id,
            project_id: project.id
        };

        conn.transaction(|transact| {
            diesel::insert_into(crate::schema::tasks::table)
                .values(&task)
                .execute(transact)?;
            diesel::insert_into(crate::schema::task_projects::table)
                .values(&task_project)
                .execute(transact)?;
            diesel::update(crate::schema::projects::table.find(&project.id))
                .set(crate::schema::projects::n_tasks.eq(project.n_tasks))
                .execute(transact)?;
            QueryResult::Ok(())
        })?;
        sender.send(task.clone()).ok();

        Ok(task)
    }

    pub fn get(conn: &mut PgConnection, task_id: Uuid) -> Option<Self> {
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
                "slug" => query = query.filter(slug.ilike(format!("%{}%", value))),
                "title" => query = query.filter(title.ilike(format!("%{}%", value))),
                "description" => query = query.filter(description.ilike(format!("%{}%", value))),
                "author" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(author_id.eq(value))
                    } else {
                        tracing::debug!("Invalid uuid");
                        continue;
                    }
                }
                "assignee" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(assignee_id.eq(value))
                    } else {
                        tracing::debug!("Invalid uuid");
                        continue;
                    }
                }
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
    pub task_id: Uuid,
    pub tag_name: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_components)]
struct TaskComponent {
    pub task_id: Uuid,
    pub component_name: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_watchers)]
struct TaskWatcher {
    pub task_id: Uuid,
    pub watcher_id: Uuid,
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

        let user = User::create(&mut conn, &mut user_tx, "test@example.com", Some("test_user"), Some("password")).expect("user");
        let mut proj = Project::create(&mut conn, &mut proj_tx, &user, "proj", "").expect("proj");
        let task = Task::create(&mut conn, &mut tx, &mut proj, "Task 1", "Do this", &user).expect("task");

        let task2 = Task::get(&mut conn, task.id).expect("task2");

        assert_eq!(task.slug, "PROJ-1");
        assert_eq!(task, task2);
        assert_eq!(proj.n_tasks, 1);
    }
}
