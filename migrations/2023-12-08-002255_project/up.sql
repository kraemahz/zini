CREATE TABLE projects (
    name VARCHAR PRIMARY KEY,
    description VARCHAR NOT NULL,
    n_tasks INTEGER DEFAULT 0
);

ALTER TABLE tasks
ADD COLUMN project VARCHAR;

ALTER TABLE tasks
ADD FOREIGN KEY (project) REFERENCES projects(name);
