use diesel::prelude::*;
use super::users::User;


#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub name: String,
    pub description: String,
    pub n_tasks: i32
}

impl Project {
    pub fn create(conn: &mut PgConnection,
                  name: &str,
                  description: &str) -> QueryResult<Self> {
        let project = Self { name: name.to_owned(),
                             description: description.to_owned(),
                             n_tasks: 0 };

        diesel::insert_into(crate::schema::projects::table)
            .values(&project)
            .execute(conn)?;

        Ok(project)
    }
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::tasks)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub assignee: Option<String>,
}

impl Task {
    pub fn create(conn: &mut PgConnection,
                  project: &mut Project,
                  title: &str,
                  description: &str,
                  author: &User) -> QueryResult<Self> {

        project.n_tasks += 1;
        let next_id = format!("{}-{}", project.name, project.n_tasks);

        let task = Self {
            id: next_id,
            project: project.name.clone(),
            title: title.to_owned(),
            description: description.to_owned(),
            author: author.username.clone(),
            assignee: None
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

        Ok(task)
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
