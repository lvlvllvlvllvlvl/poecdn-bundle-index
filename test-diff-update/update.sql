-- Differential update from 3.26.0.10 to 3.26.0.11
PRAGMA foreign_keys = on;
UPDATE version SET url = 'https://patch.poecdn.com/3.26.0.11/' WHERE id = 0;
INSERT INTO bundles (name, size) VALUES ('bundle4.bin', 4000), ('bundle5_new.bin', 5500) ON CONFLICT (name) DO UPDATE SET size=excluded.size;
INSERT INTO dirs (name, parent) VALUES ('dir4', (SELECT id FROM dirs WHERE name='dir2')), ('dir5_new', (SELECT id FROM dirs WHERE name='dir2')) ON CONFLICT (name) DO UPDATE SET parent=excluded.parent;
DELETE FROM "files" WHERE "files"."hash" IN (1003);
INSERT INTO files (hash, dir, name, bundle, offset, size) VALUES (1004, (SELECT id FROM dirs WHERE name='dir4'), 'file4.txt', (SELECT id FROM bundles WHERE name='bundle4.bin'), 0, 400), (1005, (SELECT id FROM dirs WHERE name='dir2'), 'file5_new.txt', (SELECT id FROM bundles WHERE name='bundle5_new.bin'), 100, 550) ON CONFLICT (hash) DO UPDATE SET dir=excluded.dir, name=excluded.name, bundle=excluded.bundle, offset=excluded.offset, size=excluded.size;
DELETE FROM dirs WHERE name IN ('dir3','dir5_old') AND id NOT IN (SELECT parent FROM dirs WHERE parent IS NOT NULL) AND id NOT IN (SELECT dir FROM files);
DELETE FROM bundles WHERE name IN ('bundle3.bin','bundle5_old.bin');
