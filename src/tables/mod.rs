mod flows;
mod projects;
mod tasks;
mod users;

pub use self::flows::{Flow, FlowAssignment, FlowConnection, FlowExit, FlowNode, Graph};
pub use self::projects::{ActiveProject, Project};
pub use self::tasks::{Tag, Task, TaskFlow, TaskLink, TaskLinkType, TaskUpdate};
pub use self::users::{User, UserIdAccount, UserMetadata, UserPortrait};
pub use subseq_util::tables::{DbPool, ValidationErrorMessage};

#[cfg(test)]
mod test {
    use diesel_migrations::{embed_migrations, EmbeddedMigrations};
    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");
}
