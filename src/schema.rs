// @generated automatically by Diesel CLI.

diesel::table! {
    components (name) {
        name -> Varchar,
    }
}

diesel::table! {
    flow_assignments (id) {
        id -> Uuid,
        flow_id -> Uuid,
        node_id -> Uuid,
    }
}

diesel::table! {
    flow_entries (flow_id, node_id) {
        flow_id -> Uuid,
        node_id -> Uuid,
    }
}

diesel::table! {
    flow_exits (id) {
        id -> Uuid,
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
        flow_name -> Varchar,
        description -> Text,
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

diesel::joinable!(flow_assignments -> flow_nodes (node_id));
diesel::joinable!(flow_assignments -> flows (flow_id));
diesel::joinable!(flow_entries -> flow_nodes (node_id));
diesel::joinable!(flow_entries -> flows (flow_id));
diesel::joinable!(flow_exits -> flow_nodes (node_id));
diesel::joinable!(flow_exits -> flows (flow_id));
diesel::joinable!(task_components -> components (component_name));
diesel::joinable!(task_components -> tasks (task_id));
diesel::joinable!(task_tags -> tags (tag_name));
diesel::joinable!(task_tags -> tasks (task_id));
diesel::joinable!(task_watchers -> tasks (task_id));
diesel::joinable!(task_watchers -> users (watcher_username));
diesel::joinable!(tasks -> projects (project));

diesel::allow_tables_to_appear_in_same_query!(
    components,
    flow_assignments,
    flow_entries,
    flow_exits,
    flow_node_connections,
    flow_nodes,
    flows,
    projects,
    tags,
    task_components,
    task_tags,
    task_watchers,
    tasks,
    users,
);
