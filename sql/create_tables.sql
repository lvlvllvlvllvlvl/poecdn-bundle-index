CREATE TABLE IF NOT EXISTS bundles
(
    id   INTEGER PRIMARY KEY,
    name TEXT    NOT NULL,
    size INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS dirs
(
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL,
    parent number,
    FOREIGN KEY (parent) REFERENCES dirs (id)
);

CREATE TABLE IF NOT EXISTS files
(
    hash   INTEGER PRIMARY KEY,
    dir    INTEGER,
    name   TEXT    NOT NULL,
    bundle INTEGER NOT NULL,
    offset INTEGER NOT NULL,
    size   INTEGER NOT NULL,
    FOREIGN KEY (bundle) REFERENCES bundles (id),
    FOREIGN KEY (dir) REFERENCES dirs (id)
);
