CREATE TABLE active_projects (
    user_id UUID PRIMARY KEY REFERENCES auth.users(id),
    project_id UUID REFERENCES projects(id)
);
