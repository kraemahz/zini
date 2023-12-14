-- Drop the Relationship Tables first to avoid foreign key constraints
DROP TABLE IF EXISTS flow_node_connections;
DROP TABLE IF EXISTS flow_exits;
DROP TABLE IF EXISTS flow_entries;
DROP TABLE IF EXISTS flow_assignments;

DROP TABLE IF EXISTS task_watchers;
DROP TABLE IF EXISTS task_components;
DROP TABLE IF EXISTS task_tags;
DROP TABLE IF EXISTS task_projects;

DROP TABLE IF EXISTS user_id_accounts;

-- Drop the Main Tables
DROP TABLE IF EXISTS flow_nodes;
DROP TABLE IF EXISTS flows;

DROP TABLE IF EXISTS components;
DROP TABLE IF EXISTS tags;
DROP TABLE IF EXISTS tasks;

DROP TABLE IF EXISTS projects;

DROP TABLE IF EXISTS users;
