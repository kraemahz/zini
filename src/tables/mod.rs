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

pub use subseq_util::database::{
    DbPool,
    ValidationErrorMessage,
    db_url,
    establish_connection_pool,
};

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
    use diesel::pg::PgConnection;

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
            let url = db_url("postgres", &self.host, &self.password, "postgres", false);
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
            let url = db_url("postgres", host, password, "postgres", false);
            let mut conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            let query = diesel::sql_query(&format!("CREATE DATABASE {}", database));
            query.execute(&mut conn).expect(&format!("Creating {} failed", database));

            let url = db_url("postgres", host, password, database, false);
            let mut db_conn = PgConnection::establish(&url).expect("Cannot establish database connection");
            run_migrations(&mut db_conn).expect("Migrations failed");

            Self {
                host: host.to_string(),
                password: password.to_string(),
                db_name: database.to_string(),
            }
        }

        pub fn conn(&self) -> PgConnection {
            let url = db_url("postgres", &self.host, &self.password, &self.db_name, false);
            PgConnection::establish(&url).expect("Cannot establish database connection")
        }
    }
}
