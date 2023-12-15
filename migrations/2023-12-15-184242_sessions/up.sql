CREATE TABLE sessions (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    token BYTEA NOT NULL
);
