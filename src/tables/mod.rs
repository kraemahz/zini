use diesel::prelude::*;
use diesel::pg::PgConnection;

mod users;
mod tasks;

pub use self::users::User;
pub use self::tasks::{Task, Project};

pub fn establish_connection(password: &str) -> PgConnection {
    let database_url = format!("postgres://postgres:{}@localhost/zini", password);
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}
