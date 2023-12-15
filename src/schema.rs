// @generated automatically by Diesel CLI.

diesel::table! {
    components (name) {
        name -> Varchar,
    }
}

diesel::table! {
    flow_assignments (flow_id, node_id) {
        flow_id -> Uuid,
        node_id -> Uuid,
    }
}

diesel::table! {
    flow_entries (flow_id) {
        flow_id -> Uuid,
        node_id -> Uuid,
    }
}

diesel::table! {
    flow_exits (flow_id, node_id) {
        flow_id -> Uuid,
        node_id -> Uuid,
    }
}

diesel::table! {
    flow_node_connections (from_node_id, to_node_id) {
        from_node_id -> Uuid,
        to_node_id -> Uuid,
    }
}

diesel::table! {
    flow_nodes (id) {
        id -> Uuid,
        node_name -> Varchar,
    }
}

diesel::table! {
    flows (id) {
        id -> Uuid,
        owner_id -> Uuid,
        created -> Timestamp,
        flow_name -> Varchar,
        description -> Text,
    }
}

diesel::table! {
    projects (id) {
        id -> Uuid,
        name -> Varchar,
        owner_id -> Uuid,
        created -> Timestamp,
        description -> Varchar,
        n_tasks -> Nullable<Int4>,
    }
}

diesel::table! {
    sessions (user_id) {
        user_id -> Uuid,
        token -> Nullable<Bytea>,
    }
}

diesel::table! {
    tags (name) {
        name -> Varchar,
    }
}

diesel::table! {
    task_components (task_id, component_name) {
        task_id -> Uuid,
        component_name -> Varchar,
    }
}

diesel::table! {
    task_projects (task_id, project_id) {
        task_id -> Uuid,
        project_id -> Uuid,
    }
}

diesel::table! {
    task_tags (task_id, tag_name) {
        task_id -> Uuid,
        tag_name -> Varchar,
    }
}

diesel::table! {
    task_watchers (task_id, watcher_id) {
        task_id -> Uuid,
        watcher_id -> Uuid,
    }
}

diesel::table! {
    tasks (id) {
        id -> Uuid,
        slug -> Varchar,
        created -> Timestamp,
        title -> Varchar,
        description -> Text,
        author_id -> Uuid,
        assignee_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    user_id_accounts (user_id, username) {
        user_id -> Uuid,
        username -> Varchar,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        email -> Varchar,
        created -> Timestamp,
        salt -> Nullable<Bytea>,
        hash -> Nullable<Bytea>,
    }
}

diesel::joinable!(flow_assignments -> flow_nodes (node_id));
diesel::joinable!(flow_assignments -> flows (flow_id));
diesel::joinable!(flow_entries -> flow_nodes (node_id));
diesel::joinable!(flow_entries -> flows (flow_id));
diesel::joinable!(flow_exits -> flow_nodes (node_id));
diesel::joinable!(flow_exits -> flows (flow_id));
diesel::joinable!(flows -> users (owner_id));
diesel::joinable!(projects -> users (owner_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(task_components -> components (component_name));
diesel::joinable!(task_components -> tasks (task_id));
diesel::joinable!(task_projects -> projects (project_id));
diesel::joinable!(task_projects -> tasks (task_id));
diesel::joinable!(task_tags -> tags (tag_name));
diesel::joinable!(task_tags -> tasks (task_id));
diesel::joinable!(task_watchers -> tasks (task_id));
diesel::joinable!(task_watchers -> users (watcher_id));
diesel::joinable!(user_id_accounts -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    components,
    flow_assignments,
    flow_entries,
    flow_exits,
    flow_node_connections,
    flow_nodes,
    flows,
    projects,
    sessions,
    tags,
    task_components,
    task_projects,
    task_tags,
    task_watchers,
    tasks,
    user_id_accounts,
    users,
);
