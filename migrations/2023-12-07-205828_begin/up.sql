CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

---- Users
-- Create Users Table
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email VARCHAR NOT NULL UNIQUE,
    created TIMESTAMP NOT NULL,
    salt BYTEA,
    hash BYTEA NULL,
    CONSTRAINT email_format_check CHECK (email ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$')
);

CREATE TABLE user_id_accounts (
    user_id UUID NOT NULL REFERENCES users(id),
    username VARCHAR NOT NULL UNIQUE,
    PRIMARY KEY (user_id, username)
);

---- Projects
CREATE TABLE projects (
    id UUID PRIMARY KEY,
    name VARCHAR NOT NULL UNIQUE,
    owner_id UUID NOT NULL REFERENCES users(id),
    created TIMESTAMP NOT NULL,
    description VARCHAR NOT NULL,
    n_tasks INTEGER DEFAULT 0
);

---- Tasks
-- Create Tags Table
CREATE TABLE tags (
    name VARCHAR PRIMARY KEY
);

-- Create Components Table
CREATE TABLE components (
    name VARCHAR PRIMARY KEY
);

-- Create Tasks Table
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    slug VARCHAR NOT NULL UNIQUE, 
    created TIMESTAMP NOT NULL,
    title VARCHAR NOT NULL,
    description TEXT NOT NULL,
    author_id UUID NOT NULL REFERENCES users(id),
    assignee_id UUID REFERENCES users(id)
);

-- Create Relationship Table for Tasks and Projects
CREATE TABLE task_projects (
    task_id UUID NOT NULL REFERENCES tasks(id),
    project_id UUID NOT NULL REFERENCES projects(id),
    PRIMARY KEY (task_id, project_id)
);

-- Create Relationship Table for Tasks and Tags (Many-to-Many)
CREATE TABLE task_tags (
    task_id UUID NOT NULL REFERENCES tasks(id),
    tag_name VARCHAR NOT NULL REFERENCES tags(name),
    PRIMARY KEY (task_id, tag_name)
);

-- Create Relationship Table for Tasks and Components (Many-to-Many)
CREATE TABLE task_components (
    task_id UUID NOT NULL REFERENCES tasks(id),
    component_name VARCHAR NOT NULL REFERENCES components(name),
    PRIMARY KEY (task_id, component_name)
);

-- Create Relationship Table for Tasks and Watchers (Many-to-Many)
CREATE TABLE task_watchers (
    task_id UUID NOT NULL REFERENCES tasks(id),
    watcher_id UUID NOT NULL REFERENCES users(id),
    PRIMARY KEY (task_id, watcher_id)
);

---- Flows
-- Create Flows Table
CREATE TABLE flows (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_id UUID NOT NULL REFERENCES users(id),
    created TIMESTAMP NOT NULL,
    flow_name VARCHAR NOT NULL,
    description TEXT NOT NULL
);

-- Create Flow Nodes Table
CREATE TABLE flow_nodes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    node_name VARCHAR NOT NULL
);

-- Create Flow Assignments for Flow Nodes
CREATE TABLE flow_assignments (
    flow_id UUID NOT NULL REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (flow_id, node_id)
);

-- Create Entry for Flow
CREATE TABLE flow_entries (
    flow_id UUID PRIMARY KEY REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id)
);

-- Create Exits for Flow
CREATE TABLE flow_exits (
    flow_id UUID NOT NULL REFERENCES flows(id),
    node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (flow_id, node_id)
);

-- Create Relationship Table for Flow Nodes (Many-to-Many)
CREATE TABLE flow_node_connections (
    from_node_id UUID NOT NULL REFERENCES flow_nodes(id),
    to_node_id UUID NOT NULL REFERENCES flow_nodes(id),
    PRIMARY KEY (from_node_id, to_node_id)
);

