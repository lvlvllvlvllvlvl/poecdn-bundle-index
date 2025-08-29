use anyhow::{Context, Result};
use csv::Reader;
use poecdn_bundle_index::{db, run_offline_from_index};
use sea_orm::{ConnectionTrait, Database, Statement, TransactionTrait};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("{}_{}", prefix, nanos));
    p
}

fn test_asset_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

async fn apply_sql_file(sqlite_path: &Path, sql_path: &Path) -> Result<()> {
    let db_url = format!("sqlite:{}", sqlite_path.to_string_lossy());
    let conn = Database::connect(&db_url)
        .await
        .with_context(|| format!("Failed to connect to {}", sqlite_path.display()))?;

    let sql = fs::read_to_string(sql_path)
        .with_context(|| format!("Failed to read {}", sql_path.display()))?;

    conn.execute_unprepared(sql.as_str()).await
        .with_context(|| format!("Failed to apply {} to {}", sql_path.display(), sqlite_path.display()))?;

    Ok(())
}

async fn compare_databases(prev_like: &Path, current: &Path) -> Result<()> {
    let prev_url = format!("sqlite:{}?mode=ro", prev_like.to_string_lossy());
    let curr_url = format!("sqlite:{}?mode=ro", current.to_string_lossy());

    let prev_conn = Database::connect(&prev_url).await.context("connect prev")?;
    let curr_conn = Database::connect(&curr_url).await.context("connect curr")?;

    // Compare bundles
    let mut prev_bundles = db::get_bundles(&prev_conn).await.context("bundles prev")?;
    let mut curr_bundles = db::get_bundles(&curr_conn).await.context("bundles curr")?;
    prev_bundles.sort();
    curr_bundles.sort();
    assert_eq!(
        prev_bundles, curr_bundles,
        "Bundles differ between updated and current DB"
    );

    // Compare files (with hash to ensure identity)
    let mut prev_files = db::get_files_with_hash(&prev_conn)
        .await
        .context("files prev")?;
    let mut curr_files = db::get_files_with_hash(&curr_conn)
        .await
        .context("files curr")?;
    prev_files.sort();
    curr_files.sort();
    assert_eq!(
        prev_files, curr_files,
        "Files differ between updated and current DB"
    );

    // Compare versions (full URL string)
    let prev_version = db::get_version(&prev_conn).await.context("version prev")?;
    let curr_version = db::get_version(&curr_conn).await.context("version curr")?;
    assert_eq!(
        prev_version, curr_version,
        "Version URL differs between updated and current DB"
    );

    Ok(())
}

//#[tokio::test]
async fn diff_update_3_26() -> Result<()> {
    // Inputs: local index.bin fixtures and their corresponding CDN-like URLs.
    let prev_index = test_asset_path("3.26.0.1.index.bin");
    let curr_index = test_asset_path("3.26.0.11.index.bin");

    let prev_url = "https://patch.poecdn.com/3.26.0.1/";
    let curr_url = "https://patch.poecdn.com/3.26.0.11/";

    // Create temporary output directories for each database build.
    let prev_out = unique_temp_dir("poe_prev");
    let curr_out = unique_temp_dir("poe_curr");
    fs::create_dir_all(&prev_out)?;
    fs::create_dir_all(&curr_out)?;

    // Build previous and current databases from the local index.bin files.
    run_offline_from_index(prev_url, prev_out.to_str().unwrap(), &prev_index)
        .await
        .context("building previous DB from index.bin")?;
    run_offline_from_index(curr_url, curr_out.to_str().unwrap(), &curr_index)
        .await
        .context("building current DB from index.bin")?;

    let prev_db_path = prev_out.join("bundle_index.sqlite");
    let curr_db_path = curr_out.join("bundle_index.sqlite");

    // Derive versions for generating update name and parameters.
    let prev_db_conn =
        Database::connect(&format!("sqlite:{}?mode=ro", prev_db_path.display())).await?;
    let curr_db_conn =
        Database::connect(&format!("sqlite:{}?mode=ro", curr_db_path.display())).await?;

    let prev_version_url = db::get_version(&prev_db_conn).await?;
    let curr_version_url = db::get_version(&curr_db_conn).await?;
    let from_version = db::extract_version_from_url(&prev_version_url);
    let to_version = db::extract_version_from_url(&curr_version_url);

    // Create an update.sql in a temp folder.
    let diff_out_dir = unique_temp_dir("poe_diff");
    fs::create_dir_all(&diff_out_dir)?;
    let update_sql_path =
        diff_out_dir.join(format!("update-{}-to-{}.sql", &from_version, &to_version));

    // Generate the differential update SQL.
    db::generate_differential_update(
        &prev_db_path,
        &curr_db_path,
        &update_sql_path,
        &from_version,
        &to_version,
    )
    .await
    .context("generate differential update")?;

    // Apply the update SQL to a copy of the previous DB.
    let updated_db_path = diff_out_dir.join("updated_from_prev.sqlite");
    fs::copy(&prev_db_path, &updated_db_path)
        .context("copy previous DB to create an updatable working copy")?;
    apply_sql_file(&updated_db_path, &update_sql_path)
        .await
        .context(format!("apply generated update SQL {} to previous DB copy", update_sql_path.to_string_lossy()))?;

    // Verify updated DB matches the current DB built from the 3.26 bin.
    compare_databases(&updated_db_path, &curr_db_path)
        .await
        .context("updated DB should match current DB")?;

    Ok(())
}

/// Test data for the differential update test
struct TestData {
    // Bundles that should be preserved
    preserved_bundles: Vec<(u64, &'static str, u32)>,
    // Bundles that should be removed
    removed_bundles: Vec<(u64, &'static str, u32)>,
    // Bundles that should be added
    added_bundles: Vec<(u64, &'static str, u32)>,
    // Bundles that should be modified
    modified_bundles: Vec<(u64, &'static str, u32, &'static str, u32)>,

    // Directories that should be preserved
    preserved_dirs: Vec<(u32, &'static str, Option<u32>)>,
    // Directories that should be removed
    removed_dirs: Vec<(u32, &'static str, Option<u32>)>,
    // Directories that should be added
    added_dirs: Vec<(u32, &'static str, Option<u32>)>,
    // Directories that should be modified
    modified_dirs: Vec<(u32, &'static str, Option<u32>, &'static str, Option<u32>)>,

    // Files that should be preserved
    preserved_files: Vec<(u64, u32, &'static str, u32, u32, u32)>,
    // Files that should be removed
    removed_files: Vec<(u64, u32, &'static str, u32, u32, u32)>,
    // Files that should be added
    added_files: Vec<(u64, u32, &'static str, u32, u32, u32)>,
    // Files that should be modified
    modified_files: Vec<(
        u64,
        u32,
        &'static str,
        u32,
        u32,
        u32,
        u32,
        &'static str,
        u32,
        u32,
        u32,
    )>,
}

impl TestData {
    /// Create test data for the differential update test
    fn new() -> Self {
        TestData {
            // Bundles that should be preserved (id, name, size)
            preserved_bundles: vec![(1, "bundle1.bin", 1000), (2, "bundle2.bin", 2000)],
            // Bundles that should be removed (id, name, size)
            removed_bundles: vec![(3, "bundle3.bin", 3000)],
            // Bundles that should be added (id, name, size)
            added_bundles: vec![(4, "bundle4.bin", 4000)],
            // Bundles that should be modified (id, old_name, old_size, new_name, new_size)
            modified_bundles: vec![(5, "bundle5_old.bin", 5000, "bundle5_new.bin", 5500)],

            // Directories that should be preserved (id, name, parent)
            preserved_dirs: vec![(1, "dir1", None), (2, "dir2", Some(1))],
            // Directories that should be removed (id, name, parent)
            removed_dirs: vec![(3, "dir3", Some(1))],
            // Directories that should be added (id, name, parent)
            added_dirs: vec![(4, "dir4", Some(2))],
            // Directories that should be modified (id, old_name, old_parent, new_name, new_parent)
            modified_dirs: vec![(5, "dir5_old", Some(1), "dir5_new", Some(2))],

            // Files that should be preserved (hash, dir, name, bundle, offset, size)
            preserved_files: vec![
                (1001, 1, "file1.txt", 1, 0, 100),
                (1002, 2, "file2.txt", 2, 0, 200),
            ],
            // Files that should be removed (hash, dir, name, bundle, offset, size)
            removed_files: vec![(1003, 3, "file3.txt", 3, 0, 300)],
            // Files that should be added (hash, dir, name, bundle, offset, size)
            added_files: vec![(1004, 4, "file4.txt", 4, 0, 400)],
            // Files that should be modified (hash, old_dir, old_name, old_bundle, old_offset, old_size, new_dir, new_name, new_bundle, new_offset, new_size)
            modified_files: vec![(
                1005,
                1,
                "file5_old.txt",
                5,
                0,
                500,
                2,
                "file5_new.txt",
                5,
                100,
                550,
            )],
        }
    }

    /// Create the "previous" database with initial test data
    async fn create_previous_database(&self, db_path: &Path) -> Result<()> {
        // Remove the database file if it exists
        if db_path.exists() {
            fs::remove_file(db_path)?;
        }

        // Create the database directory
        let out_dir = db_path.parent().unwrap_or(Path::new("."));
        if !out_dir.exists() {
            fs::create_dir_all(out_dir)?;
        }

        // Check if SQL files exist
        let create_tables_sql = Path::new("sql/create_tables.sql");
        let create_indexes_sql = Path::new("sql/create_indexes.sql");

        // Verify SQL files exist
        if !create_tables_sql.exists() || !create_indexes_sql.exists() {
            return Err(anyhow::anyhow!("SQL files not found"));
        }

        // Create a new database connection
        let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        let conn = Database::connect(&db_url).await?;

        // Read and execute the schema creation SQL
        let schema = fs::read_to_string(Path::new("sql/create_tables.sql"))?;
        conn.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            schema,
        ))
        .await?;

        // Read and execute the index creation SQL
        let indexes = fs::read_to_string(Path::new("sql/create_indexes.sql"))?;
        conn.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            indexes,
        ))
        .await?;

        // Check if the database file was created
        if !db_path.exists() {
            return Err(anyhow::anyhow!("Database file was not created"));
        }

        // Begin a transaction
        let tx = conn.begin().await?;

        // Insert version
        db::insert_version(&tx, "https://patch.poecdn.com/3.26.0.10/").await?;

        // Insert all test data for previous database

        // Insert bundles
        for (id, name, size) in &self.preserved_bundles {
            db::insert_bundle(&tx, *id, name, *size).await?;
        }

        for (id, name, size) in &self.removed_bundles {
            db::insert_bundle(&tx, *id, name, *size).await?;
        }

        for (id, old_name, old_size, _, _) in &self.modified_bundles {
            db::insert_bundle(&tx, *id, old_name, *old_size).await?;
        }

        // Insert directories
        for (id, name, parent) in &self.preserved_dirs {
            db::insert_dir(&tx, *id, name, *parent).await?;
        }

        for (id, name, parent) in &self.removed_dirs {
            db::insert_dir(&tx, *id, name, *parent).await?;
        }

        for (id, old_name, old_parent, _, _) in &self.modified_dirs {
            db::insert_dir(&tx, *id, old_name, *old_parent).await?;
        }

        // Insert files
        for (hash, dir, name, bundle, offset, size) in &self.preserved_files {
            db::insert_file(&tx, *hash, *dir, name, *bundle, *offset, *size).await?;
        }

        for (hash, dir, name, bundle, offset, size) in &self.removed_files {
            db::insert_file(&tx, *hash, *dir, name, *bundle, *offset, *size).await?;
        }

        for (hash, old_dir, old_name, old_bundle, old_offset, old_size, _, _, _, _, _) in
            &self.modified_files
        {
            db::insert_file(
                &tx,
                *hash,
                *old_dir,
                old_name,
                *old_bundle,
                *old_offset,
                *old_size,
            )
            .await?;
        }

        // Commit the transaction
        tx.commit().await?;

        // Verify that the version was inserted
        let version_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT url FROM version WHERE id = 0",
            vec![],
        );
        let version_row = conn.query_one(version_stmt).await?;
        if let Some(_) = version_row {
            // Version exists, no need to print it
        } else {
            return Err(anyhow::anyhow!("No version found in previous database"));
        }

        Ok(())
    }

    /// Create the "current" database with updated test data
    async fn create_current_database(&self, db_path: &Path) -> Result<()> {
        // Remove the database file if it exists
        if db_path.exists() {
            fs::remove_file(db_path)?;
        }

        // Create the database directory
        let out_dir = db_path.parent().unwrap_or(Path::new("."));
        if !out_dir.exists() {
            fs::create_dir_all(out_dir)?;
        }

        // Check if SQL files exist
        let create_tables_sql = Path::new("sql/create_tables.sql");
        let create_indexes_sql = Path::new("sql/create_indexes.sql");

        // Verify SQL files exist
        if !create_tables_sql.exists() || !create_indexes_sql.exists() {
            return Err(anyhow::anyhow!("SQL files not found"));
        }

        // Create a new database connection
        let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        let conn = Database::connect(&db_url).await?;

        // Read and execute the schema creation SQL
        let schema = fs::read_to_string(Path::new("sql/create_tables.sql"))?;
        conn.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            schema,
        ))
        .await?;

        // Read and execute the index creation SQL
        let indexes = fs::read_to_string(Path::new("sql/create_indexes.sql"))?;
        conn.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            indexes,
        ))
        .await?;

        // Check if the database file was created
        if !db_path.exists() {
            return Err(anyhow::anyhow!("Database file was not created"));
        }

        // Begin a transaction
        let tx = conn.begin().await?;

        // Insert version
        db::insert_version(&tx, "https://patch.poecdn.com/3.26.0.11/").await?;

        // Insert all test data for current database

        // Insert bundles
        for (id, name, size) in &self.preserved_bundles {
            db::insert_bundle(&tx, *id, name, *size).await?;
        }

        for (id, name, size) in &self.added_bundles {
            db::insert_bundle(&tx, *id, name, *size).await?;
        }

        for (id, _, _, new_name, new_size) in &self.modified_bundles {
            db::insert_bundle(&tx, *id, new_name, *new_size).await?;
        }

        // Insert directories
        for (id, name, parent) in &self.preserved_dirs {
            db::insert_dir(&tx, *id, name, *parent).await?;
        }

        for (id, name, parent) in &self.added_dirs {
            db::insert_dir(&tx, *id, name, *parent).await?;
        }

        for (id, _, _, new_name, new_parent) in &self.modified_dirs {
            db::insert_dir(&tx, *id, new_name, *new_parent).await?;
        }

        // Insert files
        for (hash, dir, name, bundle, offset, size) in &self.preserved_files {
            db::insert_file(&tx, *hash, *dir, name, *bundle, *offset, *size).await?;
        }

        for (hash, dir, name, bundle, offset, size) in &self.added_files {
            db::insert_file(&tx, *hash, *dir, name, *bundle, *offset, *size).await?;
        }

        for (hash, _, _, _, _, _, new_dir, new_name, new_bundle, new_offset, new_size) in
            &self.modified_files
        {
            db::insert_file(
                &tx,
                *hash,
                *new_dir,
                new_name,
                *new_bundle,
                *new_offset,
                *new_size,
            )
            .await?;
        }

        // Commit the transaction
        tx.commit().await?;

        // Verify that the version was inserted
        let version_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT url FROM version WHERE id = 0",
            vec![],
        );
        let version_row = conn.query_one(version_stmt).await?;
        if let Some(_) = version_row {
            // Version exists, no need to print it
        } else {
            return Err(anyhow::anyhow!("No version found in current database"));
        }

        Ok(())
    }
}

/// Tests the differential update functionality
#[tokio::test]
pub async fn test_differential_update() -> Result<()> {
    println!("Starting differential update test");

    // Create test directories
    let test_dir = Path::new("test-diff-update");
    println!("Test directory: {:?}", test_dir);

    if test_dir.exists() {
        println!("Removing existing test directory");
        fs::remove_dir_all(test_dir).expect("Failed to remove existing test directory");
    }

    println!("Creating test directory");
    fs::create_dir_all(test_dir).expect("Failed to create test directory");

    // Create paths for the databases
    let prev_db_path = test_dir.join("previous.sqlite");
    let current_db_path = test_dir.join("current.sqlite");
    let updated_db_path = test_dir.join("updated.sqlite");
    let update_sql_path = test_dir.join("update.sql");

    println!("Previous database path: {:?}", prev_db_path);
    println!("Current database path: {:?}", current_db_path);
    println!("Updated database path: {:?}", updated_db_path);
    println!("Update SQL path: {:?}", update_sql_path);

    // Create test data
    println!("Creating test data");
    let test_data = TestData::new();

    // Create the "previous" database
    println!("Creating previous database");
    test_data
        .create_previous_database(&prev_db_path)
        .await
        .expect("Failed to create previous database");

    // Verify the previous database was created
    assert!(prev_db_path.exists(), "Previous database was not created");

    // Create the "current" database
    println!("Creating current database");
    test_data
        .create_current_database(&current_db_path)
        .await
        .expect("Failed to create current database");

    // Verify the current database was created
    assert!(current_db_path.exists(), "Current database was not created");

    // Copy the "previous" database to create the "updated" database
    println!("Copying previous database to create updated database");
    fs::copy(&prev_db_path, &updated_db_path)
        .expect("Failed to copy previous database to updated database");

    // Verify the updated database was created
    assert!(updated_db_path.exists(), "Updated database was not created");

    // Generate the differential update SQL file
    println!("Generating differential update SQL file");
    db::generate_differential_update(
        &prev_db_path,
        &current_db_path,
        &update_sql_path,
        "3.26.0.10",
        "3.26.0.11",
    )
    .await
    .expect("Failed to generate differential update SQL file");

    // Verify the update SQL file was created
    assert!(update_sql_path.exists(), "Update SQL file was not created");

    // Apply the update to the "updated" database
    println!("Reading update SQL file");
    let update_sql = fs::read_to_string(&update_sql_path).expect("Failed to read update SQL file");

    // Print only the first few lines of the update SQL for debugging
    println!("Update SQL first few lines:");
    let sql_preview: String = update_sql.lines().take(5).collect::<Vec<&str>>().join("\n");
    println!("{}...", sql_preview);
    println!("(SQL file length: {} bytes)", update_sql.len());

    println!("Connecting to updated database");
    let updated_db_url = format!("sqlite:{}?mode=rwc", updated_db_path.to_string_lossy());
    let updated_conn = Database::connect(&updated_db_url)
        .await
        .expect("Failed to connect to updated database");

    println!("Executing update SQL");
    updated_conn
        .execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            update_sql,
        ))
        .await
        .expect("Failed to execute update SQL");

    // Connect to the "current" database for comparison
    println!("Connecting to current database");
    let current_db_url = format!("sqlite:{}?mode=rwc", current_db_path.to_string_lossy());
    let current_conn = Database::connect(&current_db_url)
        .await
        .expect("Failed to connect to current database");

    // Verify that the updated database matches the current database
    println!("Verifying that the updated database matches the current database");

    // 1. Verify bundles
    println!("Getting bundles from updated database");
    let updated_bundles = db::get_bundles(&updated_conn)
        .await
        .expect("Failed to get bundles from updated database");

    println!("Getting bundles from current database");
    let current_bundles = db::get_bundles(&current_conn)
        .await
        .expect("Failed to get bundles from current database");

    println!("Updated bundles count: {}", updated_bundles.len());
    println!("Current bundles count: {}", current_bundles.len());

    let updated_bundles_set: HashSet<_> = updated_bundles.into_iter().collect();
    let current_bundles_set: HashSet<_> = current_bundles.into_iter().collect();

    // Check that all bundles in the updated database are in the current database
    println!("Checking that all bundles in the updated database are in the current database");
    let mut missing_bundles_in_current = Vec::new();
    for bundle in &updated_bundles_set {
        if !current_bundles_set.contains(bundle) {
            missing_bundles_in_current.push((bundle.0.clone(), bundle.1));
        }
    }
    // Only print the count of missing bundles, not the entire list
    if !missing_bundles_in_current.is_empty() {
        println!(
            "Found {} bundles in updated database that are not in current database",
            missing_bundles_in_current.len()
        );
        // Print at most 5 examples
        if missing_bundles_in_current.len() <= 5 {
            println!("Missing bundles: {:?}", missing_bundles_in_current);
        } else {
            println!(
                "First 5 missing bundles: {:?}",
                &missing_bundles_in_current[0..5]
            );
        }
        assert!(
            false,
            "Found bundles in updated database that are not in current database"
        );
    }

    // Check that all bundles in the current database are in the updated database
    println!("Checking that all bundles in the current database are in the updated database");
    let mut missing_bundles_in_updated = Vec::new();
    for bundle in &current_bundles_set {
        if !updated_bundles_set.contains(bundle) {
            missing_bundles_in_updated.push((bundle.0.clone(), bundle.1));
        }
    }
    // Only print the count of missing bundles, not the entire list
    if !missing_bundles_in_updated.is_empty() {
        println!(
            "Found {} bundles in current database that are not in updated database",
            missing_bundles_in_updated.len()
        );
        // Print at most 5 examples
        if missing_bundles_in_updated.len() <= 5 {
            println!("Missing bundles: {:?}", missing_bundles_in_updated);
        } else {
            println!(
                "First 5 missing bundles: {:?}",
                &missing_bundles_in_updated[0..5]
            );
        }
        assert!(
            false,
            "Found bundles in current database that are not in updated database"
        );
    }

    // 2. Verify files
    println!("Getting files from updated database");
    let updated_files = db::get_files(&updated_conn)
        .await
        .expect("Failed to get files from updated database");

    println!("Getting files from current database");
    let current_files = db::get_files(&current_conn)
        .await
        .expect("Failed to get files from current database");

    println!("Updated files count: {}", updated_files.len());
    println!("Current files count: {}", current_files.len());

    let updated_files_set: HashSet<_> = updated_files.into_iter().collect();
    let current_files_set: HashSet<_> = current_files.into_iter().collect();

    // Check that all files in the updated database are in the current database
    println!("Checking that all files in the updated database are in the current database");
    let mut missing_files_in_current = Vec::new();
    for file in &updated_files_set {
        if !current_files_set.contains(file) {
            missing_files_in_current.push((file.0.clone(), file.1.clone(), file.2, file.3));
        }
    }
    // Only print the count of missing files, not the entire list
    if !missing_files_in_current.is_empty() {
        println!(
            "Found {} files in updated database that are not in current database",
            missing_files_in_current.len()
        );
        // Print at most 5 examples
        if missing_files_in_current.len() <= 5 {
            println!("Missing files: {:?}", missing_files_in_current);
        } else {
            println!(
                "First 5 missing files: {:?}",
                &missing_files_in_current[0..5]
            );
        }
        assert!(
            false,
            "Found files in updated database that are not in current database"
        );
    }

    // Check that all files in the current database are in the updated database
    println!("Checking that all files in the current database are in the updated database");
    let mut missing_files_in_updated = Vec::new();
    for file in &current_files_set {
        if !updated_files_set.contains(file) {
            missing_files_in_updated.push((file.0.clone(), file.1.clone(), file.2, file.3));
        }
    }
    // Only print the count of missing files, not the entire list
    if !missing_files_in_updated.is_empty() {
        println!(
            "Found {} files in current database that are not in updated database",
            missing_files_in_updated.len()
        );
        // Print at most 5 examples
        if missing_files_in_updated.len() <= 5 {
            println!("Missing files: {:?}", missing_files_in_updated);
        } else {
            println!(
                "First 5 missing files: {:?}",
                &missing_files_in_updated[0..5]
            );
        }
        assert!(
            false,
            "Found files in current database that are not in updated database"
        );
    }

    println!("Differential update test passed!");
    Ok(())
}

/// Verifies that the database content matches the CSV files using a local index.bin (offline)
#[tokio::test]
pub async fn verify_database_matches_csv() -> Result<()> {
    let out_dir_path = Path::new("test-data");

    // First, remove any existing test data to ensure a clean state
    if out_dir_path.exists() {
        fs::remove_dir_all(out_dir_path).expect("Failed to remove existing test data directory");
    }
    fs::create_dir_all(out_dir_path).expect("Failed to create test data directory");

    // Use local index.bin to generate files and database offline
    let url = "https://patch.poecdn.com/3.26.0.11/";
    let index_path = Path::new("tests/3.26.0.11.index.bin");
    assert!(
        index_path.exists(),
        "Local index.bin not found at tests/3.26.0.11.index.bin"
    );

    run_offline_from_index(url, out_dir_path.to_string_lossy().as_ref(), index_path)
        .await
        .expect("Failed to run offline process");

    // Connect to the database
    let db_path = out_dir_path.join("bundle_index.sqlite");
    assert!(db_path.exists(), "Database file was not created");

    let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
    let conn = Database::connect(&db_url)
        .await
        .expect("Failed to connect to database");

    // Get all bundles from the database
    let db_bundles = db::get_bundles(&conn)
        .await
        .expect("Failed to get bundles from database");

    // Find the subdirectory where the CSV files are generated
    // The directory structure is based on the URL: out_dir/domain/path
    let url_domain = "patch.poecdn.com";
    let url_path = "3.26.0.11"; // This is the path from the URL used above
    let csv_dir = out_dir_path.join(url_domain).join(url_path);
    assert!(csv_dir.exists(), "CSV directory was not created");

    let bundles_csv_path = csv_dir.join("bundles.csv");
    assert!(
        bundles_csv_path.exists(),
        "Bundles CSV file was not created"
    );

    // Get all bundles from the CSV file
    let mut csv_bundles = HashSet::new();
    let mut rdr = Reader::from_path(&bundles_csv_path).expect("Failed to open bundles CSV file");
    for result in rdr.records() {
        let record = result.expect("Failed to read record from bundles CSV");
        let name = record.get(0).unwrap_or("").to_string();
        let size = record
            .get(1)
            .unwrap_or("0")
            .parse::<u32>()
            .expect("Failed to parse bundle size");
        csv_bundles.insert((name, size));
    }

    // Verify that all bundles in the database are in the CSV file
    let mut missing_bundles_in_csv = Vec::new();
    for (name, size) in &db_bundles {
        if !csv_bundles.contains(&(name.clone(), *size)) {
            missing_bundles_in_csv.push((name.clone(), *size));
        }
    }
    assert!(
        missing_bundles_in_csv.is_empty(),
        "Found bundles in database that are not in CSV: {:?}",
        missing_bundles_in_csv
    );

    // Verify that all bundles in the CSV file are in the database
    let db_bundles_set: HashSet<_> = db_bundles.into_iter().collect();
    let mut missing_bundles_in_db = Vec::new();
    for (name, size) in &csv_bundles {
        if !db_bundles_set.contains(&(name.clone(), *size)) {
            missing_bundles_in_db.push((name.clone(), *size));
        }
    }

    if missing_bundles_in_csv.len() > 5 {
        assert!(
            missing_bundles_in_csv.is_empty(),
            "Found bundles in CSV that are not in database: {:?} and {} more",
            missing_bundles_in_db.first(),
            missing_bundles_in_db.len() - 1
        );
    } else {
        assert!(
            missing_bundles_in_csv.is_empty(),
            "Found bundles in CSV that are not in database: {:?}",
            missing_bundles_in_db
        );
    }

    // Get all files from the database (with hashes)
    let db_files = db::get_files_with_hash(&conn)
        .await
        .expect("Failed to get files with hash from database");

    let files_csv_path = csv_dir.join("files.csv");
    assert!(files_csv_path.exists(), "Files CSV file was not created");

    // Get all files from the CSV file (with hashes)
    let mut csv_files = HashSet::new();
    let mut rdr = Reader::from_path(&files_csv_path).expect("Failed to open files CSV file");
    for result in rdr.records() {
        let record = result.expect("Failed to read record from files CSV");
        // CSV layout: hash, file, bundle, offset, size
        let hash = record
            .get(0)
            .and_then(|s| s.parse::<u64>().ok())
            .expect("Failed to parse hash from CSV");
        let path = record.get(1).unwrap_or("").to_string();
        let bundle = record.get(2).unwrap_or("").to_string();
        let offset = record.get(3).map(|s| s.parse::<u32>().ok()).flatten();
        let size = record.get(4).map(|s| s.parse::<u32>().ok()).flatten();

        // Recompute hash and verify it matches the CSV hash
        let recomputed = murmurhash64::murmur_hash64a(path.as_bytes(), 0x1337b33f);
        assert_eq!(
            recomputed, hash,
            "Recomputed hash does not match CSV hash for {}",
            path
        );

        csv_files.insert((hash, path, bundle, offset, size));
    }

    // Verify that all files in the CSV file are in the database and hashes match
    let db_files_set: HashSet<_> = db_files.into_iter().collect();
    let mut missing_or_mismatch_in_db = Vec::new();
    for (hash, path, bundle, offset, size) in &csv_files {
        if !db_files_set.contains(&(*hash, path.clone(), bundle.clone(), *offset, *size)) {
            missing_or_mismatch_in_db.push((*hash, path.clone(), bundle.clone(), *offset, *size));
        }
    }

    if missing_or_mismatch_in_db.len() > 5 {
        assert!(
            missing_or_mismatch_in_db.is_empty(),
            "Found files in CSV that are not in database or hash mismatch: {:?} and {} more",
            missing_or_mismatch_in_db.first(),
            missing_or_mismatch_in_db.len() - 1
        )
    } else {
        assert!(
            missing_or_mismatch_in_db.is_empty(),
            "Found files in CSV that are not in database or hash mismatch: {:?}",
            missing_or_mismatch_in_db
        );
    }

    println!("Verification complete!");
    Ok(())
}
