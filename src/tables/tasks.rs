use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::api::AuthenticatedUser;

use super::{Flow, FlowNode, ValidationErrorMessage, Project, User, FlowConnection};
use super::users::UserData;


#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::task_projects)]
pub struct TaskProject {
    pub task_id: Uuid,
    pub project_id: Uuid,
}


#[derive(Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::tasks)]
pub struct Task {
    pub id: Uuid,
    pub slug: String,
    pub created: NaiveDateTime,
    pub title: String,
    pub description: String,
    pub author_id: Uuid,
    pub assignee_id: Option<Uuid>,
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id &&
            self.slug == other.slug &&
            self.created.timestamp_micros() == other.created.timestamp_micros() &&
            self.title == other.title &&
            self.description == other.description &&
            self.author_id == other.author_id &&
            self.assignee_id == other.assignee_id
    }
}

impl Task {
    pub fn create(conn: &mut PgConnection,
                  sender: &mut broadcast::Sender<Self>,
                  project: &mut Project,
                  title: &str,
                  description: &str,
                  author: &User) -> QueryResult<Self> {

        project.n_tasks += 1;
        let slug = format!("{}-{}", project.name, project.n_tasks);

        let task = Self {
            id: Uuid::new_v4(),
            slug,
            created: chrono::Utc::now().naive_utc(),
            title: title.to_owned(),
            description: description.to_owned(),
            author_id: author.id.clone(),
            assignee_id: None,
        };

        let task_project = TaskProject {
            task_id: task.id,
            project_id: project.id,
        };

        conn.transaction(|transact| {
            let default_flow_id = project.default_flow_id;
            let flow = Flow::get(transact, default_flow_id);
            let task_flow = TaskFlow {
                task_id: task.id,
                flow_id: default_flow_id,
                current_node_id: flow.map(|f| f.entry_node_id),
                order_added: 0
            };
            let watcher = TaskWatcher {
                task_id: task.id,
                watcher_id: author.id,
            };

            diesel::insert_into(crate::schema::tasks::table)
                .values(&task)
                .execute(transact)?;
            diesel::insert_into(crate::schema::task_watchers::table)
                .values(&watcher)
                .execute(transact)?;
            diesel::insert_into(crate::schema::task_flows::table)
                .values(&task_flow)
                .execute(transact)?;
            diesel::insert_into(crate::schema::task_projects::table)
                .values(&task_project)
                .execute(transact)?;
            diesel::update(crate::schema::projects::table.find(&project.id))
                .set(crate::schema::projects::n_tasks.eq(project.n_tasks))
                .execute(transact)?;
            QueryResult::Ok(())
        })?;
        sender.send(task.clone()).ok();

        Ok(task)
    }

    pub fn add_project(&self, conn: &mut PgConnection, project: &Project) -> QueryResult<()> {
        use crate::schema::task_projects;
        let task_project = TaskProject {
            task_id: self.id,
            project_id: project.id,
        };

        diesel::insert_into(task_projects::table)
            .values(&task_project)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_project(&self, conn: &mut PgConnection, project_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_projects;
       diesel::delete(task_projects::table)
            .filter(task_projects::dsl::task_id.eq(self.id))
            .filter(task_projects::dsl::project_id.eq(project_id))
            .execute(conn)?;
        Ok(())
    }

    pub fn add_tag(&self, conn: &mut PgConnection, tag: &str) -> QueryResult<()> {
        use crate::schema::task_tags;
        use crate::schema::tags;

        // Ignore errors if the tag already exists.
        let tag = Tag{name: String::from(tag)};
        diesel::insert_into(tags::table)
            .values(&tag)
            .execute(conn).ok();

        let task_tag = TaskTag {
            task_id: self.id,
            tag_name: tag.name
        };
        diesel::insert_into(task_tags::table)
            .values(&task_tag)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_tag(&self, conn: &mut PgConnection, tag: &str) -> QueryResult<()> {
        use crate::schema::task_tags;
        diesel::delete(task_tags::table)
            .filter(task_tags::dsl::tag_name.eq(tag))
            .execute(conn)?;
        Ok(())
    }

    pub fn tags(&self, conn: &mut PgConnection) -> QueryResult<Vec<String>> {
        use crate::schema::task_tags;
        use crate::schema::tags;
        task_tags::table
            .inner_join(tags::table)
            .filter(task_tags::task_id.eq(&self.id))
            .select(tags::name)
            .load::<String>(conn)
    }

    pub fn add_watcher(&self, conn: &mut PgConnection, user_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_watchers;
        let task_watcher = TaskWatcher {
            task_id: self.id,
            watcher_id: user_id
        };
        diesel::insert_into(task_watchers::table)
            .values(&task_watcher)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_watcher(&self, conn: &mut PgConnection, user_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_watchers;
       diesel::delete(task_watchers::table)
            .filter(task_watchers::dsl::task_id.eq(self.id))
            .filter(task_watchers::dsl::watcher_id.eq(user_id))
            .execute(conn)?;
        Ok(())
    }

    pub fn watchers(&self, conn: &mut PgConnection) -> QueryResult<Vec<User>> {
        use crate::schema::task_watchers;
        use crate::schema::users;
        let users = task_watchers::table
            .inner_join(users::table)
            .filter(task_watchers::task_id.eq(&self.id))
            .select(users::all_columns)
            .load::<UserData>(conn)?;
        Ok(users.into_iter().map(|user| user.into()).collect())
    }

    pub fn query(conn: &mut PgConnection,
                 query_dict: &HashMap<String, String>,
                 page: u32,
                 page_size: u32) -> Vec<Self> {
        use crate::schema::tasks::dsl::*;

        let mut query = tasks.into_boxed();
        for (key, value) in query_dict {
            match key.as_str() {
                "slug" => query = query.filter(slug.ilike(format!("%{}%", value))),
                "title" => query = query.filter(title.ilike(format!("%{}%", value))),
                "description" => query = query.filter(description.ilike(format!("%{}%", value))),
                "author" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(author_id.eq(value))
                    } else {
                        tracing::debug!("Invalid uuid");
                        continue;
                    }
                }
                "assignee" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(assignee_id.eq(value))
                    } else {
                        tracing::debug!("Invalid uuid");
                        continue;
                    }
                }
                _ => {} // Ignore unknown keys or log them if necessary
            }
        }

        let offset = page.saturating_sub(1) * page_size;

        match query
                .limit(page_size as i64)
                .offset(offset as i64)
                .load::<Task>(conn) {
            Ok(list) => list,
            Err(_) => vec![]
        }
    }

    pub fn flows(&self, conn: &mut PgConnection) -> QueryResult<Vec<TaskFlow>> {
        use crate::schema::task_flows;
        let mut flows = task_flows::table
            .filter(task_flows::dsl::task_id.eq(self.id))
            .load::<TaskFlow>(conn);
        if let Ok(flows) = flows.as_mut() {
            flows.sort_by_key(|item| item.order_added);
        }
        flows
    }

    pub fn add_link(&self, conn: &mut PgConnection, task_id: Uuid, link_type: TaskLinkType) -> QueryResult<()> {
        use crate::schema::task_links;
        let task_link = TaskLink{task_from_id: self.id,
                                 task_to_id: task_id, 
                                 link_type};
        let data: TaskLinkData = task_link.into();
        diesel::insert_into(task_links::table)
            .values(&data)
            .execute(conn)?;
        Ok(())
    }

    pub fn rm_link(&self, conn: &mut PgConnection, task_id: Uuid) -> QueryResult<()> {
        use crate::schema::task_links;
        diesel::delete(task_links::table)
            .filter(task_links::dsl::task_from_id.eq(self.id))
            .filter(task_links::dsl::task_to_id.eq(task_id))
            .execute(conn)?;
        Ok(())
    }

    pub fn transition(&self, conn: &mut PgConnection, node_id: Uuid) -> QueryResult<()> {
        for flow in self.flows(conn)? {
            if let Some(current_node_id) = flow.current_node_id {
                let valid_transitions = FlowConnection::edges(conn, current_node_id)?;
                let contains_id = valid_transitions.iter().any(|node| node.id == node_id);
                if contains_id {
                    use crate::schema::task_flows::dsl::*;
                    diesel::update(task_flows.filter(task_id.eq(self.id))
                                             .filter(flow_id.eq(flow.flow_id)))
                        .set(current_node_id.eq(Some(node_id)))
                        .execute(conn)?;
                    return Ok(())
                }
            }
        }

        let kind = diesel::result::DatabaseErrorKind::CheckViolation;
        let msg = Box::new(ValidationErrorMessage{message: format!("No valid transition to {} found", node_id),
                                                  column: "current_node_id".to_string(),
                                                  constraint_name: "valid_transition_current_node_id".to_string()});
        Err(diesel::result::Error::DatabaseError(kind, msg))
    }

    pub fn update(&mut self,
                  conn: &mut PgConnection,
                  user: AuthenticatedUser,
                  update: TaskUpdate) -> QueryResult<()> {
        match update {
            TaskUpdate::AssignOther{user_id} => self.assign_user(conn, user_id),
            TaskUpdate::AssignSelf => self.assign_user(conn, user.id()),
            TaskUpdate::ChangeDescription { description } => self.set_description(conn, &description),
            TaskUpdate::ChangeTitle { title } => self.set_title(conn, &title),
            TaskUpdate::Link { task_id, link_type } => self.add_link(conn, task_id, link_type),
            TaskUpdate::StopWatchingTask => self.rm_watcher(conn, user.id()),
            TaskUpdate::Tag { name } => self.add_tag(conn, &name),
            TaskUpdate::Transition { node_id } => self.transition(conn, node_id),
            TaskUpdate::Unassign => self.unassign_user(conn),
            TaskUpdate::WatchTask => self.add_watcher(conn, user.id()),
            TaskUpdate::Undo => Ok(()),  // TODO
            TaskUpdate::Unlink { task_id } => self.rm_link(conn, task_id),
            TaskUpdate::Untag { name } => self.rm_tag(conn, &name)
        }
    }

    fn assign_user(&mut self, conn: &mut PgConnection, user_id: Uuid) -> QueryResult<()> {
        use crate::schema::tasks::dsl;
        self.assignee_id = Some(user_id);
        diesel::update(dsl::tasks.filter(dsl::id.eq(self.id)))
            .set(dsl::assignee_id.eq(self.assignee_id))
            .execute(conn)?;
        Ok(())
    }

    fn unassign_user(&mut self, conn: &mut PgConnection) -> QueryResult<()> {
        use crate::schema::tasks::dsl;
        self.assignee_id = None;
        diesel::update(dsl::tasks.filter(dsl::id.eq(self.id)))
            .set(dsl::assignee_id.eq(self.assignee_id))
            .execute(conn)?;
        Ok(())
    }

    fn set_description(&mut self, conn: &mut PgConnection, description: &str) -> QueryResult<()> {
        use crate::schema::tasks::dsl;
        self.description = description.to_string();
        diesel::update(dsl::tasks.filter(dsl::id.eq(self.id)))
            .set(dsl::description.eq(description))
            .execute(conn)?;
        Ok(())
    }
    fn set_title(&mut self, conn: &mut PgConnection, title: &str) -> QueryResult<()> {
        use crate::schema::tasks::dsl;
        self.title = title.to_string();
        diesel::update(dsl::tasks.filter(dsl::id.eq(self.id)))
            .set(dsl::title.eq(title))
            .execute(conn)?;
        Ok(())
    }
}
crate::zini_table!(Task, crate::schema::tasks::dsl::tasks);

#[derive(Debug, Clone, Deserialize)]
pub enum TaskUpdate {
    AssignOther{user_id: Uuid},
    AssignSelf,
    ChangeDescription{description: String},
    ChangeTitle{title: String},
    Link{task_id: Uuid, link_type: TaskLinkType},
    StopWatchingTask,
    Tag{name: String},
    Transition{node_id: Uuid},
    Unassign,
    Undo,
    Unlink{task_id: Uuid},
    Untag{name: String},
    WatchTask,
}

/// Represents a label on a Task in the database
/// For future use we'll allow attaching purposes to tags
#[derive(Queryable, Insertable, Serialize)]
#[diesel(table_name = crate::schema::tags)]
pub struct Tag {
    pub name: String,
}

/// Join table for all labels on the Task
#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_tags)]
struct TaskTag {
    pub task_id: Uuid,
    pub tag_name: String,
}

/// Represents a user watching a Task for updates
#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_watchers)]
struct TaskWatcher {
    pub task_id: Uuid,
    pub watcher_id: Uuid,
}

/// Represents the state of a task in a Flow
#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_flows)]
pub struct TaskFlow {
    pub task_id: Uuid,
    pub flow_id: Uuid,
    pub current_node_id: Option<Uuid>,
    pub order_added: i32
}

/// Gets the FlowNode active from a list of TaskFlows
impl TaskFlow {
    pub fn get_active_node(conn: &mut PgConnection, flows: &[Self]) -> QueryResult<FlowNode> {
        // TODO: check other TaskFlows
        let flow = match flows.first() {
            Some(flow) => flow,
            None => {
                let kind = diesel::result::DatabaseErrorKind::CheckViolation;
                let msg = Box::new(ValidationErrorMessage{message: "Task is missing a flow".to_string(),
                                                          column: "task_id".to_string(),
                                                          constraint_name: "task_flow_required".to_string()});
                return Err(diesel::result::Error::DatabaseError(kind, msg));
            }
        };

        let node_id = match flow.current_node_id {
            Some(node_id) => node_id,
            None => {
                let kind = diesel::result::DatabaseErrorKind::CheckViolation;
                let msg = Box::new(ValidationErrorMessage{
                    message: "TaskFlow has no valid node id".to_string(),
                    column: "current_node_id".to_string(),
                    constraint_name: "task_flow_current_node_id_null".to_string()}
                );
                return Err(diesel::result::Error::DatabaseError(kind, msg));
            }
        };

        match FlowNode::get(conn, node_id) {
            Some(node) => Ok(node),
            None => {
                let kind = diesel::result::DatabaseErrorKind::CheckViolation;
                let msg = Box::new(ValidationErrorMessage{
                    message: "FlowNode is missing".to_string(),
                    column: "current_node_id".to_string(),
                    constraint_name: "task_flow_current_node_id_exists".to_string()}
                );
                return Err(diesel::result::Error::DatabaseError(kind, msg));
            }
        }
    }
}

/// Represents links between tasks for dependencies and subtask behavior
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TaskLink {
    task_from_id: Uuid,
    task_to_id: Uuid,
    link_type: TaskLinkType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskLinkType {
    SubtaskOf,
    DependsOn,
    RelatedTo
}

impl TryFrom<i32> for TaskLinkType {
    type Error = i32;
    fn try_from(entry: i32) -> Result<Self, Self::Error> {
        match entry {
            0 => Ok(Self::SubtaskOf),
            1 => Ok(Self::DependsOn),
            2 => Ok(Self::RelatedTo),
            unk => Err(unk)
        }
    }
}

impl Into<i32> for TaskLinkType {
    fn into(self) -> i32 {
        match self {
            Self::SubtaskOf => 0,
            Self::DependsOn => 1,
            Self::RelatedTo => 2,
        }
    }
}

impl TaskLinkType {
    const _LINK_TYPES: &'static [&'static str] = &["SUBTASK OF", "DEPENDS ON", "RELATED TO"];
}

#[derive(Queryable, Insertable)]
#[diesel(table_name = crate::schema::task_links)]
struct TaskLinkData {
    task_from_id: Uuid,
    task_to_id: Uuid,
    link_type: i32,
}

impl From<TaskLink> for TaskLinkData {
    fn from(link: TaskLink) -> TaskLinkData {
        let TaskLink{task_from_id, task_to_id, link_type} = link;
        TaskLinkData {
            task_from_id,
            task_to_id,
            link_type: link_type.into()
        }
    }
}

impl From<TaskLinkData> for TaskLink {
    fn from(link: TaskLinkData) -> TaskLink {
        let TaskLinkData{task_from_id, task_to_id, link_type} = link;
        TaskLink {
            task_from_id,
            task_to_id,
            link_type: link_type.try_into().unwrap()
        }
    }
}

impl TaskLink {
    pub fn create(conn: &mut PgConnection,
                  link_from: &Task,
                  link_to: &Task,
                  link_type: TaskLinkType) -> QueryResult<Self> {
        use crate::schema::task_links;
        let task_link = TaskLink{task_from_id: link_from.id,
                                 task_to_id: link_to.id, 
                                 link_type};
        let data: TaskLinkData = task_link.into();
        diesel::insert_into(task_links::table)
            .values(&data)
            .execute(conn)?;
        Ok(task_link)
    }

    pub fn get_outgoing(conn: &mut PgConnection, link_from: &Task) -> QueryResult<Vec<Self>> {
        use crate::schema::task_links;
        let links = task_links::table
            .filter(task_links::dsl::task_from_id.eq(link_from.id))
            .load::<TaskLinkData>(conn)?;
        Ok(links.into_iter().map(|link| link.into()).collect())
    }

    pub fn get_incoming(conn: &mut PgConnection, link_to: &Task) -> QueryResult<Vec<Self>> {
        use crate::schema::task_links;
        let links = task_links::table
            .filter(task_links::dsl::task_to_id.eq(link_to.id))
            .load::<TaskLinkData>(conn)?;
        Ok(links.into_iter().map(|link| link.into()).collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;

    #[test]
    #[named]
    fn test_task_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name);
        let mut conn = harness.conn(); 
        let (mut tx, _) = broadcast::channel(1);
        let (mut user_tx, _) = broadcast::channel(1);
        let (mut proj_tx, _) = broadcast::channel(1);

        let user = User::create(&mut conn, &mut user_tx, "test@example.com", Some("test_user"), Some("password")).expect("user");
        let mut proj = Project::create(
            &mut conn,
            &mut proj_tx,
            &user,
            "proj",
            "",
            None).expect("proj");
        let task = Task::create(&mut conn, &mut tx, &mut proj, "Task 1", "Do this", &user).expect("task");

        let task2 = Task::get(&mut conn, task.id).expect("task2");

        assert_eq!(task.slug, "PROJ-1");
        assert_eq!(task, task2);
        assert_eq!(proj.n_tasks, 1);
    }
}
