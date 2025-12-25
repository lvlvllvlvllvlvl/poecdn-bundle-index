-- Validation: ensure every directory is correctly linked to its parent
-- Rule: For every row in dirs with a non-NULL parent, the child's name must
-- start with the parent's name (prefix match): child.name LIKE parent.name || '%'

PRAGMA foreign_keys = ON;

-- Preview any violations (will print rows if present)
WITH violations AS (
    SELECT
        c.id           AS child_id,
        c.name         AS child_name,
        c.parent       AS parent_id,
        p.name         AS parent_name
    FROM dirs AS c
    LEFT JOIN dirs AS p ON p.id = c.parent
    WHERE c.name LIKE '%/%'                         -- subdirectories
      AND (
          p.id IS NULL                              -- missing parent
          OR c.name NOT LIKE (p.name || '/%')       -- not prefixed by parent
      )
)
SELECT * FROM violations;

-- Hard fail if any violations exist (causes non-zero exit in sqlite3 CLI)
WITH violations AS (
    SELECT 1 FROM dirs AS c
    LEFT JOIN dirs AS p ON p.id = c.parent
    WHERE c.parent IS NOT NULL
      AND (
          p.id IS NULL
          OR c.name NOT LIKE (p.name || '%')
      )
)
SELECT CASE WHEN COUNT(*) = 0 THEN 1 ELSE (1/0) END FROM violations;
