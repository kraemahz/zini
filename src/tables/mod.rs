use diesel::prelude::*;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};

mod flows;
mod users;
mod project;
mod tasks;

pub use self::flows::Flow;
pub use self::project::Project;
pub use self::tasks::Task;
pub use self::users::User;

pub fn establish_connection(password: &str) -> PgConnection {
    let database_url = format!("postgres://postgres:{}@localhost/zini", password);
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

pub type DbPool = Pool<ConnectionManager<PgConnection>>;

pub fn establish_connection_pool(database_url: &str) -> DbPool {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .build(manager)
        .expect("Failed to create pool.")
}
