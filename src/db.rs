use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, Database, DbConn, EntityTrait, QueryOrder, Statement};
use std::fs;
use std::path::Path;
use reqwest::Client;

use crate::entity::bundles;
use crate::entity::prelude::*;

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
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        schema,
    ))
    .await?;

    let indexes = fs::read_to_string(Path::new("sql/create_indexes.sql"))?;
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        indexes,
    ))
    .await?;

    Ok(conn)
}

/// Inserts a bundle into the database
pub async fn insert_bundle<C>(conn: &C, id: u64, name: &str, size: u32) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = sea_orm::Statement::from_sql_and_values(
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
    C: sea_orm::ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = sea_orm::Statement::from_sql_and_values(
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
    C: sea_orm::ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate hashes
    let stmt = sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO files (hash, dir, name, bundle, offset, size)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        vec![
            (hash as i32).into(),
            (dir as i32).into(),
            name.into(),
            (bundle as i32).into(),
            (offset as i32).into(),
            (size as i32).into(),
        ],
    );

    conn.execute(stmt).await?;

    Ok(())
}

/// Inserts a version into the database
pub async fn insert_version<C>(conn: &C, url: &str) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    // Use a raw SQL query with INSERT OR IGNORE to handle duplicate IDs
    let stmt = sea_orm::Statement::from_sql_and_values(
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
    C: sea_orm::ConnectionTrait,
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
    C: sea_orm::ConnectionTrait,
{
    // We need to use a raw SQL query for the concatenation
    let stmt = sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        r#"
        SELECT d.name || '/' || f.name AS path, b.name AS bundle_name, f.offset, f.size 
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
        // In Sea-ORM, we need to use column names instead of indices
        files.push((
            row.try_get::<String>("", "path")?,
            row.try_get::<String>("", "bundle_name")?,
            Some(row.try_get::<i32>("", "offset")? as u32),
            Some(row.try_get::<i32>("", "size")? as u32),
        ));
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
    
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to download previous database")?;
        
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download previous database: HTTP status {}",
            response.status()
        ));
    }
    
    let bytes = response.bytes().await.context("Failed to read response body")?;
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
    
    let row = conn.query_one(stmt).await?
        .ok_or_else(|| anyhow::anyhow!("Version not found in database"))?;
        
    let url = row.try_get::<String>("", "url")?;
    Ok(url)
}

/// Checks the current version in D1 database
pub async fn check_d1_version(game_type: &str) -> Result<Option<String>> {
    let client = Client::new();
    let url = format!("https://ggpk.exposed/version?poe={}", 
        if game_type == "poe1" { "1" } else { "2" }
    );
    
    println!("Checking D1 version at {}", url);
    
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to check D1 version")?;
        
    if !response.status().is_success() {
        println!("Failed to check D1 version: HTTP status {}", response.status());
        return Ok(None);
    }
    
    let version = response.text().await.context("Failed to read response body")?;
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
    println!("Generating differential update from {} to {}", from_version, to_version);
    
    // Connect to both databases
    let prev_db_url = format!("sqlite:{}?mode=ro", prev_db_path.to_string_lossy());
    let current_db_url = format!("sqlite:{}?mode=ro", current_db_path.to_string_lossy());
    
    let prev_conn = Database::connect(&prev_db_url).await?;
    let current_conn = Database::connect(&current_db_url).await?;
    
    // Create update SQL file
    let mut update_file = fs::File::create(update_sql_path)?;
    use std::io::Write;
    
    // Add header comment
    writeln!(update_file, "-- Differential update from {} to {}", from_version, to_version)?;
    writeln!(update_file, "PRAGMA defer_foreign_keys = off;")?;
    writeln!(update_file, "BEGIN TRANSACTION;")?;
    
    // Update version
    let current_version_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT url FROM version WHERE id = 0",
        vec![],
    );
    let current_version_row = current_conn.query_one(current_version_stmt).await?
        .ok_or_else(|| anyhow::anyhow!("Version not found in current database"))?;
    let current_version_url = current_version_row.try_get::<String>("", "url")?;
    
    writeln!(update_file, "UPDATE version SET url = '{}' WHERE id = 0;", current_version_url)?;
    
    // Compare and update bundles
    compare_and_update_bundles(&prev_conn, &current_conn, &mut update_file).await?;
    
    // Compare and update dirs
    compare_and_update_dirs(&prev_conn, &current_conn, &mut update_file).await?;
    
    // Compare and update files
    compare_and_update_files(&prev_conn, &current_conn, &mut update_file).await?;
    
    // End transaction
    writeln!(update_file, "COMMIT;")?;
    
    println!("Differential update SQL file generated at {:?}", update_sql_path);
    
    Ok(())
}

/// Compares and updates bundles between two databases
async fn compare_and_update_bundles(
    prev_conn: &DbConn,
    current_conn: &DbConn,
    update_file: &mut fs::File,
) -> Result<()> {
    use std::io::Write;
    use std::collections::HashMap;
    
    // Get bundles from both databases
    let prev_bundles_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id, name, size FROM bundles ORDER BY id",
        vec![],
    );
    
    let current_bundles_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id, name, size FROM bundles ORDER BY id",
        vec![],
    );
    
    let prev_bundles_rows = prev_conn.query_all(prev_bundles_stmt).await?;
    let current_bundles_rows = current_conn.query_all(current_bundles_stmt).await?;
    
    // Create maps for easier comparison
    let mut prev_bundles = HashMap::new();
    for row in prev_bundles_rows {
        let id = row.try_get::<i32>("", "id")?;
        let name = row.try_get::<String>("", "name")?;
        let size = row.try_get::<i32>("", "size")?;
        prev_bundles.insert(id, (name, size));
    }
    
    let mut current_bundles = HashMap::new();
    for row in current_bundles_rows {
        let id = row.try_get::<i32>("", "id")?;
        let name = row.try_get::<String>("", "name")?;
        let size = row.try_get::<i32>("", "size")?;
        current_bundles.insert(id, (name, size));
    }
    
    // Find deleted bundles
    for id in prev_bundles.keys() {
        if !current_bundles.contains_key(id) {
            writeln!(update_file, "DELETE FROM bundles WHERE id = {};", id)?;
        }
    }
    
    // Find added or modified bundles
    for (id, (name, size)) in &current_bundles {
        if !prev_bundles.contains_key(id) {
            // Added bundle
            writeln!(
                update_file,
                "INSERT INTO bundles (id, name, size) VALUES ({}, '{}', {});",
                id, name, size
            )?;
        } else {
            let (prev_name, prev_size) = &prev_bundles[id];
            if prev_name != name || prev_size != size {
                // Modified bundle
                writeln!(
                    update_file,
                    "UPDATE bundles SET name = '{}', size = {} WHERE id = {};",
                    name, size, id
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
    use std::io::Write;
    use std::collections::HashMap;
    
    // Get dirs from both databases using raw SQL to handle NULL values properly
    let prev_dirs_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id, name, parent FROM dirs ORDER BY id",
        vec![],
    );
    
    let current_dirs_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id, name, parent FROM dirs ORDER BY id",
        vec![],
    );
    
    let prev_dirs_rows = prev_conn.query_all(prev_dirs_stmt).await?;
    let current_dirs_rows = current_conn.query_all(current_dirs_stmt).await?;
    
    // Create maps for easier comparison
    let mut prev_dirs = HashMap::new();
    for row in prev_dirs_rows {
        let id = row.try_get::<i32>("", "id")?;
        let name = row.try_get::<String>("", "name")?;
        
        // Try to get parent, but it might be NULL
        let parent = match row.try_get::<i32>("", "parent") {
            Ok(val) => Some(val),
            Err(_) => None, // If error (likely NULL), set to None
        };
        
        prev_dirs.insert(id, (name, parent));
    }
    
    let mut current_dirs = HashMap::new();
    for row in current_dirs_rows {
        let id = row.try_get::<i32>("", "id")?;
        let name = row.try_get::<String>("", "name")?;
        
        // Try to get parent, but it might be NULL
        let parent = match row.try_get::<i32>("", "parent") {
            Ok(val) => Some(val),
            Err(_) => None, // If error (likely NULL), set to None
        };
        
        current_dirs.insert(id, (name, parent));
    }
    
    // Find deleted dirs
    for id in prev_dirs.keys() {
        if !current_dirs.contains_key(id) {
            writeln!(update_file, "DELETE FROM dirs WHERE id = {};", id)?;
        }
    }
    
    // Find added or modified dirs
    for (id, (name, parent)) in &current_dirs {
        if !prev_dirs.contains_key(id) {
            // Added dir
            if let Some(parent_id) = parent {
                writeln!(
                    update_file,
                    "INSERT INTO dirs (id, name, parent) VALUES ({}, '{}', {});",
                    id, name, parent_id
                )?;
            } else {
                writeln!(
                    update_file,
                    "INSERT INTO dirs (id, name, parent) VALUES ({}, '{}', NULL);",
                    id, name
                )?;
            }
        } else {
            let (prev_name, prev_parent) = &prev_dirs[id];
            if prev_name != name || prev_parent != parent {
                // Modified dir
                if let Some(parent_id) = parent {
                    writeln!(
                        update_file,
                        "UPDATE dirs SET name = '{}', parent = {} WHERE id = {};",
                        name, parent_id, id
                    )?;
                } else {
                    writeln!(
                        update_file,
                        "UPDATE dirs SET name = '{}', parent = NULL WHERE id = {};",
                        name, id
                    )?;
                }
            }
        }
    }
    
    Ok(())
}

/// Compares and updates files between two databases
async fn compare_and_update_files(
    prev_conn: &DbConn,
    current_conn: &DbConn,
    update_file: &mut fs::File,
) -> Result<()> {
    use std::io::Write;
    use std::collections::HashMap;
    
    // Get files from both databases
    let prev_files_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT hash, dir, name, bundle, offset, size FROM files ORDER BY hash",
        vec![],
    );
    
    let current_files_stmt = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT hash, dir, name, bundle, offset, size FROM files ORDER BY hash",
        vec![],
    );
    
    let prev_files_rows = prev_conn.query_all(prev_files_stmt).await?;
    let current_files_rows = current_conn.query_all(current_files_stmt).await?;
    
    // Create maps for easier comparison
    let mut prev_files = HashMap::new();
    for row in prev_files_rows {
        let hash = row.try_get::<i32>("", "hash")?;
        let dir = row.try_get::<i32>("", "dir")?;
        let name = row.try_get::<String>("", "name")?;
        let bundle = row.try_get::<i32>("", "bundle")?;
        let offset = row.try_get::<i32>("", "offset")?;
        let size = row.try_get::<i32>("", "size")?;
        prev_files.insert(hash, (dir, name, bundle, offset, size));
    }
    
    let mut current_files = HashMap::new();
    for row in current_files_rows {
        let hash = row.try_get::<i32>("", "hash")?;
        let dir = row.try_get::<i32>("", "dir")?;
        let name = row.try_get::<String>("", "name")?;
        let bundle = row.try_get::<i32>("", "bundle")?;
        let offset = row.try_get::<i32>("", "offset")?;
        let size = row.try_get::<i32>("", "size")?;
        current_files.insert(hash, (dir, name, bundle, offset, size));
    }
    
    // Find deleted files
    for hash in prev_files.keys() {
        if !current_files.contains_key(hash) {
            writeln!(update_file, "DELETE FROM files WHERE hash = {};", hash)?;
        }
    }
    
    // Find added or modified files
    for (hash, (dir, name, bundle, offset, size)) in &current_files {
        if !prev_files.contains_key(hash) {
            // Added file
            writeln!(
                update_file,
                "INSERT INTO files (hash, dir, name, bundle, offset, size) VALUES ({}, {}, '{}', {}, {}, {});",
                hash, dir, name, bundle, offset, size
            )?;
        } else {
            let (prev_dir, prev_name, prev_bundle, prev_offset, prev_size) = &prev_files[hash];
            if prev_dir != dir || prev_name != name || prev_bundle != bundle || prev_offset != offset || prev_size != size {
                // Modified file
                writeln!(
                    update_file,
                    "UPDATE files SET dir = {}, name = '{}', bundle = {}, offset = {}, size = {} WHERE hash = {};",
                    dir, name, bundle, offset, size, hash
                )?;
            }
        }
    }
    
    Ok(())
}
