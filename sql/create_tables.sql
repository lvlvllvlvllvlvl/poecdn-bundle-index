CREATE TABLE IF NOT EXISTS version (
    id  INTEGER PRIMARY KEY NOT NULL,
    url TEXT                NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS bundles (
    id   INTEGER PRIMARY KEY NOT NULL,
    name TEXT                NOT NULL,
    size INTEGER             NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS dirs (
    id     INTEGER PRIMARY KEY NOT NULL,
    name   TEXT                NOT NULL,
    parent INTEGER,
    FOREIGN KEY (parent) REFERENCES dirs (id)
) STRICT;

CREATE TABLE IF NOT EXISTS files (
    hash   INTEGER PRIMARY KEY NOT NULL,
    dir    INTEGER,
    name   TEXT                NOT NULL,
    bundle INTEGER             NOT NULL,
    offset INTEGER             NOT NULL,
    size   INTEGER             NOT NULL,
    FOREIGN KEY (bundle) REFERENCES bundles (id),
    FOREIGN KEY (dir) REFERENCES dirs (id)
) STRICT;
