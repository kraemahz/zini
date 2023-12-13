use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};

mod flows;
mod users;
mod project;
mod tasks;

pub use self::flows::{Flow, FlowAssignment, FlowNode, FlowConnection, FlowEntry, FlowExit, Graph};
pub use self::project::Project;
pub use self::tasks::Task;
pub use self::users::User;

pub fn db_url(host: &str, password: &str, database: &str) -> String {
    format!("postgres://postgres:{}@{}/{}", password, host, database)
}

pub type DbPool = Pool<ConnectionManager<PgConnection>>;

pub fn establish_connection_pool(database_url: &str) -> DbPool {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .build(manager)
        .expect("Failed to create pool.")
}

#[cfg(test)]
pub(self) mod harness {
    use super::*;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use diesel::pg::Pg;
    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");

    pub fn to_pg_db_name(name: &str) -> String {
        let mut db_name = String::new();
    
        // Ensure the name starts with an underscore if it doesn't start with a letter
        if name.chars().next().map_or(true, |c| !c.is_ascii_alphabetic()) {
            db_name.push('_');
        }

        // Convert function name to lowercase and replace invalid characters
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() {
                db_name.push(ch.to_ascii_lowercase());
            } else {
                db_name.push('_');
            }
        }

        // Truncate if length exceeds 63 characters
        let max_length = 63;
        if db_name.len() > max_length {
            db_name.truncate(max_length);
        }

        db_name
    }

    fn run_migrations(connection: &mut impl MigrationHarness<Pg>) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        connection.run_pending_migrations(MIGRATIONS)?;
        Ok(())
    }

    pub struct DbHarness {
        host: String,
        password: String,
        db_name: String,
    }

    impl Drop for DbHarness {
        fn drop(&mut self) {
            let url = db_url(&self.host, &self.password, "postgres");
            let mut conn = PgConnection::establish(&url).expect("Cannot establish database connection");

            let disconnect_users = format!("SELECT pg_terminate_backend(pid)
                                           FROM pg_stat_activity
                                           WHERE datname = '{}';", self.db_name);
            if diesel::sql_query(&disconnect_users).execute(&mut conn).is_err() {
                eprintln!("Failed to drop database {}", self.db_name);
                return;
            }

            let drop_db = format!("DROP DATABASE {}", self.db_name);
            if diesel::sql_query(&drop_db).execute(&mut conn).is_err() {
                eprintln!("Failed to drop database {}", self.db_name);
                return;
            }
        }
    }

    impl DbHarness {
        pub fn new(host: &str, password: &str, database: &str) -> Self {
            let url = db_url(host, password, "postgres");
            let mut conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            let query = diesel::sql_query(&format!("CREATE DATABASE {}", database));
            query.execute(&mut conn).expect(&format!("Creating {} failed", database));

            let url = db_url(host, password, database);
            let mut db_conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            run_migrations(&mut db_conn).expect("Migrations failed");

            Self {
                host: host.to_string(),
                password: password.to_string(),
                db_name: database.to_string(),
            }
        }

        pub fn conn(&self) -> PgConnection {
            let url = db_url(&self.host, &self.password, &self.db_name);
            PgConnection::establish(&url).expect("Cannot establish database connection")
        }
    }
}
