CREATE UNIQUE INDEX bundle_name ON bundles (name);
CREATE UNIQUE INDEX dir_name ON dirs (name);
CREATE INDEX dir_parent ON dirs (parent);
CREATE INDEX file_path ON files (dir, name);
