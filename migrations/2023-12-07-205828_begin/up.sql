-- Create Users Table
CREATE TABLE users (
    username VARCHAR PRIMARY KEY,
    created TIMESTAMP NOT NULL,
    email VARCHAR NOT NULL
    CONSTRAINT email_format_check CHECK (email ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$')
);

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
    id VARCHAR PRIMARY KEY,
    title VARCHAR NOT NULL,
    description TEXT NOT NULL,
    author VARCHAR NOT NULL REFERENCES users(username),
    assignee VARCHAR REFERENCES users(username)
);

-- Create Relationship Table for Tasks and Tags (Many-to-Many)
CREATE TABLE task_tags (
    task_id VARCHAR NOT NULL REFERENCES tasks(id),
    tag_name VARCHAR NOT NULL REFERENCES tags(name),
    PRIMARY KEY (task_id, tag_name)
);

-- Create Relationship Table for Tasks and Components (Many-to-Many)
CREATE TABLE task_components (
    task_id VARCHAR NOT NULL REFERENCES tasks(id),
    component_name VARCHAR NOT NULL REFERENCES components(name),
    PRIMARY KEY (task_id, component_name)
);

-- Create Relationship Table for Tasks and Watchers (Many-to-Many)
CREATE TABLE task_watchers (
    task_id VARCHAR NOT NULL REFERENCES tasks(id),
    watcher_username VARCHAR NOT NULL REFERENCES users(username),
    PRIMARY KEY (task_id, watcher_username)
);

