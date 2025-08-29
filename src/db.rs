use crate::entity::prelude::*;
use crate::entity::{bundles, dirs};
use anyhow::{Context, Result};
use itertools::Itertools;
use reqwest::Client;
use sea_orm::DatabaseBackend::Sqlite;
use sea_orm::{
    ColumnTrait, ConnectionTrait, Database, DbConn, EntityTrait, QueryFilter, QueryOrder,
    QueryTrait, Statement,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const SQLITE_MAX_VARIABLE_NUMBER: usize = 999;

/// Creates a new SQLite database and initializes it with the schema
pub async fn create_database(out_dir: &Path) -> Result<DbConn> {
    let db_path = out_dir.join("bundle_index.sqlite");

    // Remove existing database if it exists
    if db_path.exists() {
        fs::remove_file(&db_path)?;
    }

    // Create a new database
    let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
    let conn = Database::connect(&db_url).await?;

    // Read and execute the schema creation SQL
    let schema = fs::read_to_string(Path::new("sql/create_tables.sql"))?;
    conn.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        schema,
    ))
    .await?;

    let indexes = fs::read_to_string(Path::new("sql/create_indexes.sql"))?;
    conn.execute(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        indexes,
    ))
    .await?;

    Ok(conn)
}

/// Inserts a bundle into the database
pub async fn insert_bundle<C>(conn: &C, id: u64, name: &str, size: u32) -> Result<()>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO bundles (id, name, size)
        VALUES (?, ?, ?)
        "#,
        vec![(id as i32).into(), name.into(), (size as i32).into()],
    );

    conn.execute(stmt).await?;

    Ok(())
}

/// Inserts a directory into the database
pub async fn insert_dir<C>(conn: &C, id: u32, name: &str, parent: Option<u32>) -> Result<()>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO dirs (id, name, parent)
        VALUES (?, ?, ?)
        "#,
        vec![
            (id as i32).into(),
            name.into(),
            parent.map(|p| p as i32).into(),
        ],
    );

    conn.execute(stmt).await?;

    Ok(())
}

/// Inserts a file into the database
pub async fn insert_file<C>(
    conn: &C,
    hash: u64,
    dir: u32,
    name: &str,
    bundle: u32,
    offset: u32,
    size: u32,
) -> Result<()>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate hashes
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO files (hash, dir, name, bundle, offset, size)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        vec![
            (hash as i64).into(),
            (dir as i64).into(),
            name.into(),
            (bundle as i64).into(),
            (offset as i64).into(),
            (size as i64).into(),
        ],
    );

    conn.execute(stmt).await?;

    Ok(())
}

/// Inserts a version into the database
pub async fn insert_version<C>(conn: &C, url: &str) -> Result<()>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO version (id, url)
        VALUES (?, ?)
        "#,
        vec![0.into(), url.into()],
    );

    conn.execute(stmt).await?;

    Ok(())
}

/// Gets all bundles from the database
pub async fn get_bundles<C>(conn: &C) -> Result<Vec<(String, u32)>>
where
    C: ConnectionTrait,
{
    let bundles = Bundles::find()
        .order_by_asc(bundles::Column::Id)
        .all(conn)
        .await?;

    let result = bundles
        .into_iter()
        .map(|b| (b.name, b.size as u32))
        .collect();

    Ok(result)
}

/// Gets all files from the database
pub async fn get_files<C>(conn: &C) -> Result<Vec<(String, String, Option<u32>, Option<u32>)>>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query and also fetch bundle size to mimic CSV semantics for offset/size
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        SELECT 
            d.name || '/' || f.name AS path,
            b.name AS bundle_name,
            b.size AS bundle_size,
            f.offset,
            f.size 
        FROM files f 
        JOIN bundles b ON f.bundle = b.id 
        JOIN dirs d ON f.dir = d.id 
        ORDER BY d.name, f.name
        "#,
        vec![],
    );

    let query_result = conn.query_all(stmt).await?;

    let mut files = Vec::new();
    for row in query_result {
        // Fetch values
        let path: String = row.try_get("", "path")?;
        let bundle_name: String = row.try_get("", "bundle_name")?;
        let bundle_size: i32 = row.try_get("", "bundle_size")?;
        let offset: i32 = row.try_get("", "offset")?;
        let size: i32 = row.try_get("", "size")?;

        // Match CSV behavior: if offset == 0 and size == bundle_size, represent as None
        let (offset_opt, size_opt) = if offset == 0 && size == bundle_size {
            (None, None)
        } else {
            (Some(offset as u32), Some(size as u32))
        };

        files.push((path, bundle_name, offset_opt, size_opt));
    }

    Ok(files)
}

/// Gets all files from the database including their hashes
pub async fn get_files_with_hash<C>(
    conn: &C,
) -> Result<Vec<(u64, String, String, Option<u32>, Option<u32>)>>
where
    C: ConnectionTrait,
{
    // Use a raw SQL query and also fetch bundle size to mimic CSV semantics for offset/size
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        SELECT 
            f.hash AS hash,
            d.name || '/' || f.name AS path,
            b.name AS bundle_name,
            b.size AS bundle_size,
            f.offset,
            f.size 
        FROM files f 
        JOIN bundles b ON f.bundle = b.id 
        JOIN dirs d ON f.dir = d.id 
        ORDER BY d.name, f.name
        "#,
        vec![],
    );

    let query_result = conn.query_all(stmt).await?;

    let mut files = Vec::new();
    for row in query_result {
        // Fetch values
        let hash_i64: i64 = row.try_get("", "hash")?;
        let hash = hash_i64 as u64;
        let path: String = row.try_get("", "path")?;
        let bundle_name: String = row.try_get("", "bundle_name")?;
        let bundle_size: i32 = row.try_get("", "bundle_size")?;
        let offset: i32 = row.try_get("", "offset")?;
        let size: i32 = row.try_get("", "size")?;

        // Match CSV behavior: if offset == 0 and size == bundle_size, represent as None
        let (offset_opt, size_opt) = if offset == 0 && size == bundle_size {
            (None, None)
        } else {
            (Some(offset as u32), Some(size as u32))
        };

        files.push((hash, path, bundle_name, offset_opt, size_opt));
    }

    Ok(files)
}

/// Downloads the previous version's database from GitHub Pages
pub async fn download_previous_database(game_type: &str, output_path: &Path) -> Result<()> {
    let client = Client::new();
    let url = format!(
        "https://lvlvllvlvllvlvl.github.io/poecdn-bundle-index/{}/bundle_index.sqlite",
        game_type
    );

    println!("Downloading previous database from {}", url);

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to download previous database")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download previous database: HTTP status {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;
    fs::write(output_path, bytes).context("Failed to write database file")?;

    println!("Previous database downloaded to {:?}", output_path);

    Ok(())
}

/// Gets the version from a database
pub async fn get_version(conn: &DbConn) -> Result<String> {
    let stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT url FROM version WHERE id = 0",
        vec![],
    );

    let row = conn
        .query_one(stmt)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Version not found in database"))?;

    let url = row.try_get::<String>("", "url")?;
    Ok(url)
}

/// Checks the current version in D1 database
pub async fn check_d1_version(game_type: &str) -> Result<Option<String>> {
    let client = Client::new();
    let url = format!(
        "https://ggpk.exposed/version?poe={}",
        if game_type == "poe1" { "1" } else { "2" }
    );

    println!("Checking D1 version at {}", url);

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to check D1 version")?;

    if !response.status().is_success() {
        println!(
            "Failed to check D1 version: HTTP status {}",
            response.status()
        );
        return Ok(None);
    }

    let version = response
        .text()
        .await
        .context("Failed to read response body")?;
    if version.is_empty() {
        println!("D1 version is empty");
        return Ok(None);
    }

    println!("D1 version: {}", version);
    Ok(Some(version))
}

/// Extracts version string from URL
pub fn extract_version_from_url(url: &str) -> String {
    // Extract version from URL like "https://patch.poecdn.com/3.26.0.10/"
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    parts.last().unwrap_or(&"unknown").to_string()
}

/// Generates a differential update SQL file by comparing two databases
pub async fn generate_differential_update(
    prev_db_path: &Path,
    current_db_path: &Path,
    update_sql_path: &Path,
    from_version: &str,
    to_version: &str,
) -> Result<()> {
    println!(
        "Generating differential update from {} to {}",
        from_version, to_version
    );

    // Connect to both databases
    let prev_db_url = format!("sqlite:{}?mode=ro", prev_db_path.to_string_lossy());
    let current_db_url = format!("sqlite:{}?mode=ro", current_db_path.to_string_lossy());

    let prev_conn = Database::connect(&prev_db_url).await?;
    let current_conn = Database::connect(&current_db_url).await?;

    // Create update SQL file
    let mut update_file = fs::File::create(update_sql_path)?;
    use std::io::Write;

    // Add header comment
    writeln!(
        update_file,
        "-- Differential update from {} to {}",
        from_version, to_version
    )?;
    writeln!(update_file, "PRAGMA foreign_keys = off;")?;
    writeln!(update_file, "BEGIN TRANSACTION;")?;

    // Update version
    let current_version_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT url FROM version WHERE id = 0",
        vec![],
    );
    let current_version_row = current_conn
        .query_one(current_version_stmt)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Version not found in current database"))?;
    let current_version_url = current_version_row.try_get::<String>("", "url")?;

    writeln!(
        update_file,
        "UPDATE version SET url = '{}' WHERE id = 0;",
        current_version_url
    )?;

    // Process to ensure referenced entities exist before files operations:
    // 1. First, compare and update bundles (names are unique)
    // 2. Then, compare and update directories (handle parent relations by name)
    // 3. Finally, compare and update files (reference bundles/dirs by name)

    // Compare and update bundles
    compare_and_update_bundles(&prev_conn, &current_conn, &mut update_file).await?;

    // Compare and update dirs
    compare_and_update_dirs(&prev_conn, &current_conn, &mut update_file).await?;

    // Compare and update files
    compare_and_update_files(&prev_conn, &current_conn, &mut update_file).await?;

    // End transaction
    writeln!(update_file, "COMMIT;")?;
    writeln!(update_file, "PRAGMA foreign_keys = on;")?;

    println!(
        "Differential update SQL file generated at {:?}",
        update_sql_path
    );

    Ok(())
}

/// Compares and updates bundles between two databases
async fn compare_and_update_bundles(
    prev_conn: &DbConn,
    current_conn: &DbConn,
    update_file: &mut fs::File,
) -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Write;

    // Helper to escape single quotes
    fn esc(s: &str) -> String {
        s.replace("'", "''")
    }

    // Get bundles from both databases by name
    let prev_bundles_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT name, size FROM bundles ORDER BY name",
        vec![],
    );

    let current_bundles_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT name, size FROM bundles ORDER BY name",
        vec![],
    );

    let prev_bundles_rows = prev_conn.query_all(prev_bundles_stmt).await?;
    let current_bundles_rows = current_conn.query_all(current_bundles_stmt).await?;

    // Create maps keyed by name for easier comparison
    let mut prev_bundles: BTreeMap<String, i32> = BTreeMap::new();
    for row in prev_bundles_rows {
        let name = row.try_get::<String>("", "name")?;
        let size = row.try_get::<i32>("", "size")?;
        prev_bundles.insert(name, size);
    }

    let mut current_bundles: BTreeMap<String, i32> = BTreeMap::new();
    for row in current_bundles_rows {
        let name = row.try_get::<String>("", "name")?;
        let size = row.try_get::<i32>("", "size")?;
        current_bundles.insert(name, size);
    }

    // Find deleted bundles (by name)
    for (name, _) in &prev_bundles {
        if !current_bundles.contains_key(name) {
            writeln!(
                update_file,
                "DELETE FROM bundles WHERE name = '{}';",
                esc(name)
            )?;
        }
    }

    // Find added or modified bundles
    for (name, size) in &current_bundles {
        if !prev_bundles.contains_key(name) {
            // Added bundle: do not specify id, let SQLite generate it
            writeln!(
                update_file,
                "INSERT INTO bundles (name, size) VALUES ('{}', {});",
                esc(name),
                size
            )?;
        } else {
            let prev_size = prev_bundles.get(name).unwrap();
            if prev_size != size {
                // Modified bundle: use WHERE name
                writeln!(
                    update_file,
                    "UPDATE bundles SET size = {} WHERE name = '{}';",
                    size,
                    esc(name)
                )?;
            }
        }
    }

    Ok(())
}

/// Compares and updates dirs between two databases
async fn compare_and_update_dirs(
    prev_conn: &DbConn,
    current_conn: &DbConn,
    update_file: &mut fs::File,
) -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Write;

    // Helper to escape single quotes
    fn esc(s: &str) -> String {
        s.replace("'", "''")
    }

    // Get dirs with parent names for both databases
    let prev_dirs_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT d.name AS name, p.name AS parent_name FROM dirs d LEFT JOIN dirs p ON d.parent = p.id ORDER BY d.name",
        vec![],
    );

    let current_dirs_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT d.name AS name, p.name AS parent_name FROM dirs d LEFT JOIN dirs p ON d.parent = p.id ORDER BY d.name",
        vec![],
    );

    let prev_dirs_rows = prev_conn.query_all(prev_dirs_stmt).await?;
    let current_dirs_rows = current_conn.query_all(current_dirs_stmt).await?;

    // Create maps keyed by directory name to parent name (Option<String>)
    let mut prev_dirs: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in prev_dirs_rows {
        let name = row.try_get::<String>("", "name")?;
        let parent_name: Option<String> = match row.try_get::<String>("", "parent_name") {
            Ok(val) => Some(val),
            Err(_) => None,
        };
        prev_dirs.insert(name, parent_name);
    }

    let mut current_dirs: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in current_dirs_rows {
        let name = row.try_get::<String>("", "name")?;
        let parent_name: Option<String> = match row.try_get::<String>("", "parent_name") {
            Ok(val) => Some(val),
            Err(_) => None,
        };
        current_dirs.insert(name, parent_name);
    }

    for chunk in &prev_dirs
        .iter()
        .map(|(name, _)| name)
        .filter(|name| !current_dirs.contains_key(*name))
        .chunks(SQLITE_MAX_VARIABLE_NUMBER)
    {
        let stmt = Dirs::delete_many()
            .filter(dirs::Column::Name.is_in(chunk))
            .build(Sqlite);
        writeln!(update_file, "{};", stmt)?;
    }

    let mut to_update = Vec::new();
    for (name, parent_name) in &current_dirs {
        add_dir_to_update(name, parent_name, &mut to_update, &prev_dirs, &current_dirs);
    }

    // Find added or modified dirs
    for chunk in to_update.chunks(SQLITE_MAX_VARIABLE_NUMBER / 2) {
        if chunk.len() == 0 {
            continue;
        }
        write!(update_file, "INSERT INTO dirs (name, parent) VALUES",)?;
        let mut comma = "";
        for (name, parent_name) in chunk {
            if let Some(pn) = parent_name {
                write!(
                    update_file,
                    "{} ('{}', (SELECT id FROM dirs WHERE name='{}'))",
                    comma,
                    esc(name),
                    esc(pn)
                )?;
            } else {
                write!(update_file, "{} ('{}', NULL)", comma, esc(name))?;
            }
            comma = ",";
        }
        writeln!(
            update_file,
            " ON CONFLICT (name) DO UPDATE SET parent=excluded.parent;",
        )?;
    }

    Ok(())
}

fn add_dir_to_update(
    name: &String,
    parent_name: &Option<String>,
    to_update: &mut Vec<(String, Option<String>)>,
    prev_dirs: &BTreeMap<String, Option<String>>,
    current_dirs: &BTreeMap<String, Option<String>>,
) {
    if let Some(Some(p)) = current_dirs.get(name) {
        // Ensure that parent is added before any child
        add_dir_to_update(
            p,
            current_dirs.get(p).unwrap(),
            to_update,
            prev_dirs,
            current_dirs,
        );
    }
    if prev_dirs
        .get(name)
        .is_none_or(|prev_parent| prev_parent != parent_name)
        && !to_update.iter().any(|(n, _)| n == name)
    {
        to_update.push((name.clone(), parent_name.clone()));
    }
}

/// Compares and updates files between two databases
async fn compare_and_update_files(
    prev_conn: &DbConn,
    current_conn: &DbConn,
    update_file: &mut fs::File,
) -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Write;

    // Helper to escape single quotes
    fn esc(s: &str) -> String {
        s.replace("'", "''")
    }

    // Get files from both databases with dir and bundle names
    let prev_files_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT f.hash AS hash, d.name AS dir_name, f.name AS file_name, b.name AS bundle_name, f.offset AS offset, f.size AS size FROM files f JOIN dirs d ON f.dir = d.id JOIN bundles b ON f.bundle = b.id ORDER BY f.hash",
        vec![],
    );

    let current_files_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT f.hash AS hash, d.name AS dir_name, f.name AS file_name, b.name AS bundle_name, f.offset AS offset, f.size AS size FROM files f JOIN dirs d ON f.dir = d.id JOIN bundles b ON f.bundle = b.id ORDER BY f.hash",
        vec![],
    );

    let prev_files_rows = prev_conn.query_all(prev_files_stmt).await?;
    let current_files_rows = current_conn.query_all(current_files_stmt).await?;

    // Create maps for easier comparison keyed by hash
    let mut prev_files: BTreeMap<i64, (String, String, String, i32, i32)> = BTreeMap::new();
    for row in prev_files_rows {
        let hash = row.try_get::<i64>("", "hash")?;
        let dir_name = row.try_get::<String>("", "dir_name")?;
        let file_name = row.try_get::<String>("", "file_name")?;
        let bundle_name = row.try_get::<String>("", "bundle_name")?;
        let offset = row.try_get::<i32>("", "offset")?;
        let size = row.try_get::<i32>("", "size")?;
        prev_files.insert(hash, (dir_name, file_name, bundle_name, offset, size));
    }

    let mut current_files: BTreeMap<i64, (String, String, String, i32, i32)> = BTreeMap::new();
    for row in current_files_rows {
        let hash = row.try_get::<i64>("", "hash")?;
        let dir_name = row.try_get::<String>("", "dir_name")?;
        let file_name = row.try_get::<String>("", "file_name")?;
        let bundle_name = row.try_get::<String>("", "bundle_name")?;
        let offset = row.try_get::<i32>("", "offset")?;
        let size = row.try_get::<i32>("", "size")?;
        current_files.insert(hash, (dir_name, file_name, bundle_name, offset, size));
    }

    // Find deleted files
    for hash in prev_files.keys() {
        if !current_files.contains_key(hash) {
            writeln!(update_file, "DELETE FROM files WHERE hash = {};", hash)?;
        }
    }

    // Find added or modified files
    for (hash, (dir_name, file_name, bundle_name, offset, size)) in &current_files {
        if !prev_files.contains_key(hash) {
            // Added file: set dir and bundle by name lookups
            writeln!(
                update_file,
                "INSERT INTO files (hash, dir, name, bundle, offset, size) VALUES ({}, (SELECT id FROM dirs WHERE name='{}'), '{}', (SELECT id FROM bundles WHERE name='{}'), {}, {});",
                hash,
                esc(dir_name),
                esc(file_name),
                esc(bundle_name),
                offset,
                size
            )?;
        } else {
            let (prev_dir_name, prev_file_name, prev_bundle_name, prev_offset, prev_size) =
                &prev_files[hash];
            if prev_dir_name != dir_name
                || prev_file_name != file_name
                || prev_bundle_name != bundle_name
                || prev_offset != offset
                || prev_size != size
            {
                // Modified file: update by name lookups
                writeln!(
                    update_file,
                    "UPDATE files SET dir = (SELECT id FROM dirs WHERE name='{}'), name = '{}', bundle = (SELECT id FROM bundles WHERE name='{}'), offset = {}, size = {} WHERE hash = {};",
                    esc(dir_name),
                    esc(file_name),
                    esc(bundle_name),
                    offset,
                    size,
                    hash
                )?;
            }
        }
    }

    Ok(())
}
