-- Drop the Relationship Tables first to avoid foreign key constraints
DROP TABLE IF EXISTS task_watchers;
DROP TABLE IF EXISTS task_components;
DROP TABLE IF EXISTS task_tags;

-- Drop the Main Tables
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS components;
DROP TABLE IF EXISTS tags;
DROP TABLE IF EXISTS users;
