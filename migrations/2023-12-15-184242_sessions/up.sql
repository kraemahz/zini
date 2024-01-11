CREATE TABLE sessions (
    user_id UUID PRIMARY KEY REFERENCES auth.users(id),
    token BYTEA NOT NULL
);
