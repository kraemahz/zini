ALTER TABLE tasks
ADD FOREIGN KEY (author) REFERENCES users(username);
