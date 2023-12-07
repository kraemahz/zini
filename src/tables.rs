use diesel::prelude::*;
use diesel::pg::PgConnection;
use chrono::NaiveDateTime;

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::users)]
struct User {
    pub username: String,
    pub created: NaiveDateTime,
    pub email: String,
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::tasks)]
struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub author: String, // assuming username
    pub assignee: Option<String>,
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

pub fn establish_connection(password: &str) -> PgConnection {
    let database_url = format!("postgres://postgres:{}@localhost/zini", password);
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}
