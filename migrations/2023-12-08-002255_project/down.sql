ALTER TABLE tasks
DROP CONSTRAINT tasks_project_fkey;

ALTER TABLE tasks
DROP COLUMN project;

DROP TABLE projects;
