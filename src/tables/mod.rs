mod flows;
mod projects;
mod tasks;
mod users;

pub use self::flows::{Flow, FlowAssignment, FlowNode, FlowConnection, FlowExit, Graph};
pub use self::projects::Project;
pub use self::tasks::{Task, Tag, TaskFlow, TaskLink, TaskLinkType, TaskUpdate};
pub use self::users::{User, UserIdAccount};
pub use subseq_util::tables::{ DbPool, ValidationErrorMessage };

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
mod test {
    use diesel_migrations::{EmbeddedMigrations, embed_migrations};
    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");
}
