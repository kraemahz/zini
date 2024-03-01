CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id),
    task_id UUID NOT NULL REFERENCES tasks(id),
    name VARCHAR NOT NULL,
    created_id UUID NOT NULL REFERENCES auth.users(id),
    assignee_id UUID NOT NULL REFERENCES auth.users(id)
);

CREATE TABLE job_results (
    job_id UUID PRIMARY KEY REFERENCES jobs(id),
    completion_time TIMESTAMP NOT NULL,
    succeeded BOOLEAN NOT NULL,
    job_log VARCHAR NOT NULL
);

CREATE TABLE awaiting_help (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id),
    request VARCHAR NOT NULL
);

CREATE TABLE help_resolution (
    help_id UUID PRIMARY KEY REFERENCES awaiting_help(id),
    result VARCHAR NOT NULL
);

CREATE TABLE help_resolution_actions (
    id UUID PRIMARY KEY,
    help_id UUID NOT NULL REFERENCES awaiting_help(id),
    action_taken VARCHAR NOT NULL
);

CREATE TABLE help_resolution_files (
    id UUID PRIMARY KEY,
    action_id UUID NOT NULL REFERENCES help_resolution_actions(id),
    file_name VARCHAR NOT NULL
);
