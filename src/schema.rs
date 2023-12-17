// @generated automatically by Diesel CLI.

diesel::table! {
    default_project_tags (project_id, tag_name) {
        project_id -> Uuid,
        tag_name -> Varchar,
    }
}

diesel::table! {
    flow_assignments (flow_id, node_id) {
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
        entry_node_id -> Uuid,
    }
}

diesel::table! {
    link_types (id) {
        id -> Int4,
        link_name -> Varchar,
    }
}

diesel::table! {
    projects (id) {
        id -> Uuid,
        name -> Varchar,
        owner_id -> Uuid,
        created -> Timestamp,
        description -> Varchar,
        n_tasks -> Int4,
        default_flow_id -> Uuid,
    }
}

diesel::table! {
    sessions (user_id) {
        user_id -> Uuid,
        token -> Bytea,
    }
}

diesel::table! {
    tags (name) {
        name -> Varchar,
    }
}

diesel::table! {
    task_flows (task_id, flow_id) {
        task_id -> Uuid,
        flow_id -> Uuid,
        current_node_id -> Nullable<Uuid>,
        order_added -> Int4,
    }
}

diesel::table! {
    task_links (task_from_id, task_to_id) {
        task_from_id -> Uuid,
        task_to_id -> Uuid,
        link_type -> Int4,
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

diesel::joinable!(default_project_tags -> projects (project_id));
diesel::joinable!(default_project_tags -> tags (tag_name));
diesel::joinable!(flow_assignments -> flow_nodes (node_id));
diesel::joinable!(flow_assignments -> flows (flow_id));
diesel::joinable!(flow_exits -> flow_nodes (node_id));
diesel::joinable!(flow_exits -> flows (flow_id));
diesel::joinable!(flows -> flow_nodes (entry_node_id));
diesel::joinable!(flows -> users (owner_id));
diesel::joinable!(projects -> users (owner_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(task_flows -> flow_nodes (current_node_id));
diesel::joinable!(task_flows -> flows (flow_id));
diesel::joinable!(task_flows -> tasks (task_id));
diesel::joinable!(task_links -> link_types (link_type));
diesel::joinable!(task_projects -> projects (project_id));
diesel::joinable!(task_projects -> tasks (task_id));
diesel::joinable!(task_tags -> tags (tag_name));
diesel::joinable!(task_tags -> tasks (task_id));
diesel::joinable!(task_watchers -> tasks (task_id));
diesel::joinable!(task_watchers -> users (watcher_id));
diesel::joinable!(user_id_accounts -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    default_project_tags,
    flow_assignments,
    flow_exits,
    flow_node_connections,
    flow_nodes,
    flows,
    link_types,
    projects,
    sessions,
    tags,
    task_flows,
    task_links,
    task_projects,
    task_tags,
    task_watchers,
    tasks,
    user_id_accounts,
    users,
);
