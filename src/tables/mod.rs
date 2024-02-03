mod flows;
mod projects;
mod tasks;
mod users;

pub use self::flows::{Flow, FlowAssignment, FlowNode, FlowConnection, FlowExit, Graph};
pub use self::projects::{ActiveProject, Project};
pub use self::tasks::{Task, Tag, TaskFlow, TaskLink, TaskLinkType, TaskUpdate};
pub use self::users::{User, UserIdAccount, UserMetadata, UserPortraits};
pub use subseq_util::tables::{ DbPool, ValidationErrorMessage };

#[cfg(test)]
mod test {
    use diesel_migrations::{EmbeddedMigrations, embed_migrations};
    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");
}
