// @generated automatically by Diesel CLI.

diesel::table! {
    components (name) {
        name -> Varchar,
    }
}

diesel::table! {
    projects (name) {
        name -> Varchar,
        description -> Varchar,
        n_tasks -> Nullable<Int4>,
    }
}

diesel::table! {
    tags (name) {
        name -> Varchar,
    }
}

diesel::table! {
    task_components (task_id, component_name) {
        task_id -> Varchar,
        component_name -> Varchar,
    }
}

diesel::table! {
    task_tags (task_id, tag_name) {
        task_id -> Varchar,
        tag_name -> Varchar,
    }
}

diesel::table! {
    task_watchers (task_id, watcher_username) {
        task_id -> Varchar,
        watcher_username -> Varchar,
    }
}

diesel::table! {
    tasks (id) {
        id -> Varchar,
        title -> Varchar,
        description -> Text,
        author -> Varchar,
        assignee -> Nullable<Varchar>,
        project -> Nullable<Varchar>,
    }
}

diesel::table! {
    users (username) {
        username -> Varchar,
        created -> Timestamp,
        email -> Varchar,
    }
}

diesel::joinable!(task_components -> components (component_name));
diesel::joinable!(task_components -> tasks (task_id));
diesel::joinable!(task_tags -> tags (tag_name));
diesel::joinable!(task_tags -> tasks (task_id));
diesel::joinable!(task_watchers -> tasks (task_id));
diesel::joinable!(task_watchers -> users (watcher_username));
diesel::joinable!(tasks -> projects (project));

diesel::allow_tables_to_appear_in_same_query!(
    components,
    projects,
    tags,
    task_components,
    task_tags,
    task_watchers,
    tasks,
    users,
);
