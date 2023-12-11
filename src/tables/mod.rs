use diesel::prelude::*;
use diesel::pg::PgConnection;

mod users;
mod project;
mod tasks;

pub use self::users::User;
pub use self::project::Project;
pub use self::tasks::Task;

pub fn establish_connection(password: &str) -> PgConnection {
    let database_url = format!("postgres://postgres:{}@localhost/zini", password);
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}
