use std::time::Duration;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};

mod flows;
pub(crate) mod users;
mod projects;
mod sessions;
mod tasks;

pub use self::flows::{Flow, FlowAssignment, FlowNode, FlowConnection, FlowExit, Graph};
pub use self::projects::Project;
pub use self::tasks::{Task, Tag, TaskFlow, TaskLink, TaskLinkType, TaskUpdate};
pub use self::sessions::Session;
pub use self::users::User;

pub fn db_url(username: &str, host: &str, password: &str, database: &str, ssl: bool) -> String {
    let ssl_string = "?sslmode=require";
    format!("postgres://{}:{}@{}/{}{}",
            username,
            password,
            host,
            database,
            if ssl {ssl_string} else {""})
}

pub type DbPool = Pool<ConnectionManager<PgConnection>>;
const DB_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn establish_connection_pool(database_url: &str) -> DbPool {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool_async = tokio::task::spawn_blocking(|| Pool::builder().build(manager));
    match tokio::time::timeout(DB_TIMEOUT, pool_async).await {
        Ok(Ok(pool)) => pool.expect("Could not establish database connection"),
        Ok(Err(err)) => panic!("Database connection task failed: {:?}", err),
        Err(_) => panic!("Database connection timed out after {} secs", DB_TIMEOUT.as_secs())
    }
}

pub struct ValidationErrorMessage {
    pub message: String,
    pub column: String,
    pub constraint_name: String
}

impl diesel::result::DatabaseErrorInformation for ValidationErrorMessage {
    fn message(&self) -> &str {
        &self.message
    }
    fn details(&self) -> Option<&str> {
        None
    }
    fn hint(&self) -> Option<&str> {
        None
    }
    fn table_name(&self) -> Option<&str> {
        None
    }
    fn column_name(&self) -> Option<&str> {
        Some(&self.column)
    }
    fn constraint_name(&self) -> Option<&str> {
        Some(&self.constraint_name)
    }
    fn statement_position(&self) -> Option<i32> {
        None
    }
}

#[macro_export]
macro_rules! zini_table {
    ($struct_name:ident, $table:path) => {
        impl $struct_name {
            pub fn list(conn: &mut PgConnection,
                        page: u32,
                        page_size: u32) -> Vec<Self> {
                let offset = page.saturating_sub(1) * page_size;
                match $table
                        .limit(page_size as i64)
                        .offset(offset as i64)
                        .load::<Self>(conn) {
                    Ok(list) => list,
                    Err(err) => {
                        tracing::warn!("DB List Query Failed: {:?}", err);
                        vec![]
                    }
                }
            }

            pub fn get(conn: &mut PgConnection, id: Uuid) -> Option<Self> {
                $table.find(id).get_result::<Self>(conn).optional().ok()?
            }
        }
    };
}

#[cfg(test)]
pub(self) mod harness {
    use super::*;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use diesel::prelude::*;
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
            let url = db_url("postgres", &self.host, &self.password, "postgres");
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
            let url = db_url("postgres", host, password, "postgres");
            let mut conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            let query = diesel::sql_query(&format!("CREATE DATABASE {}", database));
            query.execute(&mut conn).expect(&format!("Creating {} failed", database));

            let url = db_url("postgres", host, password, database);
            let mut db_conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            run_migrations(&mut db_conn).expect("Migrations failed");

            Self {
                host: host.to_string(),
                password: password.to_string(),
                db_name: database.to_string(),
            }
        }

        pub fn conn(&self) -> PgConnection {
            let url = db_url("postgres", &self.host, &self.password, &self.db_name);
            PgConnection::establish(&url).expect("Cannot establish database connection")
        }
    }
}
