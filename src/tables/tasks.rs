use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::Flow;
use super::projects::Project;
use super::users::User;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::task_projects)]
pub struct TaskProject {
    pub task_id: Uuid,
    pub project_id: Uuid,
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
            project_id: project.id,
        };

        conn.transaction(|transact| {
            let default_flow_id = project.default_flow_id;
            let flow = Flow::get(transact, default_flow_id);
            let task_flow = TaskFlow {
                task_id: task.id,
                flow_id: default_flow_id,
                current_node_id: flow.map(|f| f.entry_node_id),
                order_added: 0
            };
            diesel::insert_into(crate::schema::task_flows::table)
                .values(&task_flow)
                .execute(transact)?;

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

    pub fn add_project(&self, conn: &mut PgConnection, project: &Project) -> QueryResult<()> {
        use crate::schema::task_projects;
        let task_project = TaskProject {
            task_id: self.id,
            project_id: project.id,
        };

        diesel::insert_into(task_projects::table)
            .values(&task_project)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_project(&self, conn: &mut PgConnection, project_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_projects;
       diesel::delete(task_projects::table)
            .filter(task_projects::dsl::task_id.eq(self.id))
            .filter(task_projects::dsl::project_id.eq(project_id))
            .execute(conn)?;
        Ok(())
    }

    pub fn add_tag(&self, conn: &mut PgConnection, tag: &str) -> QueryResult<()> {
        use crate::schema::task_tags;
        use crate::schema::tags;

        // Ignore errors if the tag already exists.
        let tag = Tag{name: String::from(tag)};
        diesel::insert_into(tags::table)
            .values(&tag)
            .execute(conn).ok();

        let task_tag = TaskTag {
            task_id: self.id,
            tag_name: tag.name
        };
        diesel::insert_into(task_tags::table)
            .values(&task_tag)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_tag(&self, conn: &mut PgConnection, tag: &str) -> QueryResult<()> {
        use crate::schema::task_tags;
        diesel::delete(task_tags::table)
            .filter(task_tags::dsl::tag_name.eq(tag))
            .execute(conn)?;
        Ok(())
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

    pub fn add_watcher(&self, conn: &mut PgConnection, user: &User) -> QueryResult<()> {
        use crate::schema::task_watchers;
        let task_watcher = TaskWatcher {
            task_id: self.id,
            watcher_id: user.id
        };
        diesel::insert_into(task_watchers::table)
            .values(&task_watcher)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_watcher(&self, conn: &mut PgConnection, user_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_watchers;
       diesel::delete(task_watchers::table)
            .filter(task_watchers::dsl::task_id.eq(self.id))
            .filter(task_watchers::dsl::watcher_id.eq(user_id))
            .execute(conn)?;
        Ok(())
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
                 page: u32,
                 page_size: u32) -> Vec<Self> {
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

        match query
                .limit(page_size as i64)
                .offset(offset as i64)
                .load::<Task>(conn) {
            Ok(list) => list,
            Err(_) => vec![]
        }
    }

    pub fn flows(&self, conn: &mut PgConnection) -> QueryResult<Vec<TaskFlow>> {
        use crate::schema::task_flows;
        let mut flows = task_flows::table
            .filter(task_flows::dsl::task_id.eq(self.id))
            .load::<TaskFlow>(conn);
        if let Ok(flows) = flows.as_mut() {
            flows.sort_by_key(|item| item.order_added);
        }
        flows
    }
}

crate::zini_table!(Task, crate::schema::tasks::dsl::tasks);

#[derive(Queryable, Insertable, Serialize)]
#[diesel(table_name = crate::schema::tags)]
pub struct Tag {
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
#[diesel(table_name = crate::schema::task_watchers)]
struct TaskWatcher {
    pub task_id: Uuid,
    pub watcher_id: Uuid,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_flows)]
pub struct TaskFlow {
    pub task_id: Uuid,
    pub flow_id: Uuid,
    pub current_node_id: Option<Uuid>,
    pub order_added: i32
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
        let mut proj = Project::create(
            &mut conn,
            &mut proj_tx,
            &user,
            "proj",
            "",
            None).expect("proj");
        let task = Task::create(&mut conn, &mut tx, &mut proj, "Task 1", "Do this", &user).expect("task");

        let task2 = Task::get(&mut conn, task.id).expect("task2");

        assert_eq!(task.slug, "PROJ-1");
        assert_eq!(task, task2);
        assert_eq!(proj.n_tasks, 1);
    }
}
