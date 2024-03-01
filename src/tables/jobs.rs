use std::collections::HashMap;
use std::str::FromStr;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::jobs)]
pub struct Job {
    pub id: Uuid,
    pub project_id: Uuid,
    pub task_id: Uuid,
    pub name: String,
    pub created_id: Uuid,
    pub assignee_id: Uuid,
}

impl Job {
    pub fn create(conn: &mut PgConnection,
                  job_id: Uuid,
                  project_id: Uuid,
                  task_id: Uuid,
                  name: String,
                  created_id: Uuid,
                  assignee_id: Uuid) -> QueryResult<Self> {
        use crate::schema::jobs;
        let job = Self {
            id: job_id,
            project_id,
            task_id,
            name,
            created_id,
            assignee_id
        };
        diesel::insert_into(jobs::table)
            .values(&job)
            .execute(conn)?;
        Ok(job)
    }

    pub fn query(conn: &mut PgConnection,
                 user_id: Uuid,
                 query_dict: &HashMap<String, String>,
                 page: u32,
                 page_size: u32) -> QueryResult<Vec<(Self, Option<JobResult>)>> {
        use crate::schema::jobs::dsl::*;
        use crate::schema::job_results;

        let mut query = crate::schema::jobs::table
            .left_join(crate::schema::job_results::table)
            .into_boxed();

        for (key, value) in query_dict {
            match key.as_str() {
                "project_id" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(project_id.eq(value));
                    }
                }
                "created_id" => if let Ok(value) = Uuid::try_parse(value) {
                    query = query.filter(created_id.eq(value));
                } else if value == "self" {
                    query = query.filter(created_id.eq(user_id))
                }
                "running" => {
                    if let Ok(value) = bool::from_str(&value) {
                        if value {
                            query = query.filter(job_results::dsl::job_id.is_null());
                        } else {
                            query = query.filter(job_results::dsl::job_id.is_not_null());
                        }
                    }
                }
                "assignee_id" => if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(assignee_id.eq(value));
                    } else if value == "self" {
                        query = query.filter(assignee_id.eq(user_id))
                }
                "task_id" => {
                    if let Ok(value) = Uuid::try_parse(value) {
                        query = query.filter(task_id.eq(value));
                    }
                }
                "name" => query = query.filter(name.ilike(format!("%{}%", value))),
                _ => {} // Ignore unknown keys or log them if necessary
            }
        }

        let offset = page.saturating_sub(1) * page_size;
        Ok(query
            .limit(page_size as i64)
            .offset(offset as i64)
            .load::<(Job, Option<JobResult>)>(conn)?)
    }

    pub fn get(conn: &mut PgConnection,
               id: Uuid) -> Option<(Self, Option<JobResult>)> {

        crate::schema::jobs::table
            .find(id)
            .left_join(crate::schema::job_results::table)
            .get_result::<(Self, Option<JobResult>)>(conn)
            .optional().ok()?
    }
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::job_results)]
pub struct JobResult {
    pub job_id: Uuid,
    pub completion_time: NaiveDateTime,
    pub succeeded: bool,
    pub job_log: String
}

impl JobResult {
    pub fn create(conn: &mut PgConnection,
                  job: &Job,
                  completion_time: NaiveDateTime,
                  succeeded: bool,
                  job_log: String) -> QueryResult<Self> {
        use crate::schema::job_results;
        let job_result = Self {
            job_id: job.id,
            completion_time,
            succeeded,
            job_log
        };
        diesel::insert_into(job_results::table)
            .values(&job_result)
            .execute(conn)?;
        Ok(job_result)
    }
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::awaiting_help)]
pub struct AwaitingHelp {
    pub id: Uuid,
    pub job_id: Uuid,
    pub request: String,
}

impl AwaitingHelp {
    pub fn create(conn: &mut PgConnection,
                  job_id: Uuid,
                  request: String) -> QueryResult<Self> {
        use crate::schema::awaiting_help;
        let help = AwaitingHelp {
            id: Uuid::new_v4(),
            job_id,
            request
        };
        diesel::insert_into(awaiting_help::table)
            .values(&help)
            .execute(conn)?;
        Ok(help)
    }

    pub fn next_open_help(conn: &mut PgConnection,
                          job: &Job) -> Option<Self> {
        use crate::schema::awaiting_help;
        use crate::schema::help_resolution;
        let job_id = job.id;
        awaiting_help::table
            .left_join(help_resolution::table.on(awaiting_help::id.eq(help_resolution::help_id)))
            .filter(awaiting_help::job_id.eq(job_id))
            .filter(help_resolution::help_id.is_null())
            .select(awaiting_help::all_columns)
            .first(conn)
            .optional()
            .ok()?
    }
}
#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::help_resolution)]
pub struct HelpResolution {
    pub help_id: Uuid,
    pub result: String
}

impl HelpResolution {
    pub fn create(conn: &mut PgConnection,
                  help: &AwaitingHelp,
                  result: String) -> QueryResult<Self> {
        use crate::schema::help_resolution;
        let help = Self {
            help_id: help.id,
            result
        };
        diesel::insert_into(help_resolution::table)
            .values(&help)
            .execute(conn)?;
        Ok(help)
    }

    pub fn get(conn: &mut PgConnection, help_id: Uuid) -> Option<Self> {
        use crate::schema::help_resolution::dsl;
        dsl::help_resolution
            .find(help_id)
            .get_result::<Self>(conn)
            .optional()
            .ok()?
    }
}

#[derive(Serialize)]
pub struct DenormalizedHelpAction {
    pub help_id: Uuid,
    pub action_taken: String,
    pub files_changed: Vec<String>
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::help_resolution_actions)]
pub struct HelpResolutionAction {
    pub id: Uuid,
    pub help_id: Uuid,
    pub action_taken: String
}

impl HelpResolutionAction {
    pub fn create(conn: &mut PgConnection,
                  help: &AwaitingHelp,
                  action_taken: String,
                  files_changed: Vec<String>) -> QueryResult<Self> {
        use crate::schema::help_resolution_actions;
        let help = Self {
            id: Uuid::new_v4(),
            help_id: help.id,
            action_taken
        };
        diesel::insert_into(help_resolution_actions::table)
            .values(&help)
            .execute(conn)?;

        for file_changed in files_changed {
            HelpResolutionFiles::create(conn, &help, file_changed)?;
        }
        Ok(help)
    }

    pub fn list(conn: &mut PgConnection, help_id: Uuid) -> QueryResult<Vec<DenormalizedHelpAction>> {
        use crate::schema::help_resolution_actions;
        let actions = help_resolution_actions::table
            .filter(help_resolution_actions::help_id.eq(help_id))
            .load::<Self>(conn)?;

        let mut denorm_list = vec![];
        for action in actions {
            let files = HelpResolutionFiles::list(conn, action.id)?;
            let action = DenormalizedHelpAction {
                help_id: action.id,
                action_taken: action.action_taken,
                files_changed: files
            };
            denorm_list.push(action);
        }
        Ok(denorm_list)
    }

    pub fn update(&mut self, conn: &mut PgConnection, action_taken: String, files_changed: Vec<String>) -> QueryResult<()> {
        use crate::schema::help_resolution_actions;
        self.action_taken = action_taken;
        diesel::update(help_resolution_actions::table.find(&self.id))
            .set(help_resolution_actions::action_taken.eq(&self.action_taken))
            .execute(conn)?;
        HelpResolutionFiles::replace_files_for_action(conn, self, files_changed)?;
        Ok(())
    }

    pub fn get(conn: &mut PgConnection, id: Uuid) -> Option<Self> {
        use crate::schema::help_resolution_actions::dsl;
        dsl::help_resolution_actions
            .find(id)
            .get_result::<Self>(conn)
            .optional()
            .ok()?
    }

    pub fn delete(&self, conn: &mut PgConnection) -> QueryResult<()> {
        use crate::schema::help_resolution_actions;
        use crate::schema::help_resolution_files;

        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::delete(help_resolution_files::table.filter(help_resolution_files::action_id.eq(self.id)))
                .execute(conn)?;

            diesel::delete(help_resolution_actions::table)
                .filter(help_resolution_actions::dsl::id.eq(self.id))
                .execute(conn)?;
            Ok(())
        })
    }
}

#[derive(PartialEq, Queryable, Insertable, Clone, Debug, Serialize)]
#[diesel(table_name = crate::schema::help_resolution_files)]
pub struct HelpResolutionFiles {
    pub id: Uuid,
    pub action_id: Uuid,
    pub file_name: String,
}

impl HelpResolutionFiles {
    pub fn create(conn: &mut PgConnection,
                  action: &HelpResolutionAction,
                  file_changed: String) -> QueryResult<Self> {
        use crate::schema::help_resolution_files;
        let help = Self {
            id: Uuid::new_v4(),
            action_id: action.id,
            file_name: file_changed
        };
        diesel::insert_into(help_resolution_files::table)
            .values(&help)
            .execute(conn)?;
        Ok(help)
    }

    fn list(conn: &mut PgConnection, action_id: Uuid) -> QueryResult<Vec<String>> {
        use crate::schema::help_resolution_files;
        Ok(help_resolution_files::table
            .filter(help_resolution_files::action_id.eq(action_id))
            .load::<Self>(conn)?
            .into_iter()
            .map(|file| file.file_name)
            .collect())
    }

    fn replace_files_for_action(
        conn: &mut PgConnection,
        help_action: &HelpResolutionAction,
        new_files: Vec<String>,
    ) -> QueryResult<()> {
        use crate::schema::help_resolution_files;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            // Delete existing entries matching action_id
            diesel::delete(help_resolution_files::table.filter(help_resolution_files::action_id.eq(help_action.id)))
                .execute(conn)?;

            // Insert new entries from Vec<String>
            for file_name in new_files {
                HelpResolutionFiles::create(conn, &help_action, file_name)?;
            }
            Ok(())
        })
    }
}
