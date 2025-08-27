-- Differential update from 3.26.0.10 to 3.26.0.11
PRAGMA foreign_keys = off;
BEGIN TRANSACTION;
UPDATE version SET url = 'https://patch.poecdn.com/3.26.0.11/' WHERE id = 0;
DELETE FROM bundles WHERE name = 'bundle5_old.bin';
DELETE FROM bundles WHERE name = 'bundle3.bin';
INSERT INTO bundles (name, size) VALUES ('bundle4.bin', 4000);
INSERT INTO bundles (name, size) VALUES ('bundle5_new.bin', 5500);
DELETE FROM dirs WHERE name = 'dir3';
DELETE FROM dirs WHERE name = 'dir5_old';
INSERT INTO dirs (name, parent) VALUES ('dir4', (SELECT id FROM dirs WHERE name='dir2'));
INSERT INTO dirs (name, parent) VALUES ('dir5_new', (SELECT id FROM dirs WHERE name='dir2'));
DELETE FROM files WHERE hash = 1003;
INSERT INTO files (hash, dir, name, bundle, offset, size) VALUES (1004, (SELECT id FROM dirs WHERE name='dir4'), 'file4.txt', (SELECT id FROM bundles WHERE name='bundle4.bin'), 0, 400);
UPDATE files SET dir = (SELECT id FROM dirs WHERE name='dir2'), name = 'file5_new.txt', bundle = (SELECT id FROM bundles WHERE name='bundle5_new.bin'), offset = 100, size = 550 WHERE hash = 1005;
COMMIT;
PRAGMA foreign_keys = on;
