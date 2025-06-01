CREATE INDEX dir_name ON dirs (name);
CREATE INDEX dir_parent ON dirs (parent);
CREATE INDEX file_path ON files (dir, name);
