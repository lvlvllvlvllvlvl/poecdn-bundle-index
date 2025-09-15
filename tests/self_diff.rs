use anyhow::{Context, Result};
use poecdn_bundle_index::{db, run_offline_from_index};
use sea_orm::{ConnectionTrait, Database};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let start = SystemTime::now();
    let since = start.duration_since(UNIX_EPOCH).unwrap().as_nanos();
    PathBuf::from(format!("tmp/{}_{}", prefix, since))
}

fn test_asset_path(name: &str) -> PathBuf {
    PathBuf::from("tests").join(name)
}

async fn apply_sql_file(sqlite_path: &Path, sql: &str) -> Result<()> {
    let db_url = format!("sqlite:{}?mode=rwc", sqlite_path.to_string_lossy());
    let conn = Database::connect(&db_url)
        .await
        .context("connect for apply")?;
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        sql.to_string(),
    ))
    .await
    .context("execute update sql")?;
    Ok(())
}

async fn compare_databases(a: &Path, b: &Path) -> Result<()> {
    let a_url = format!("sqlite:{}?mode=ro", a.to_string_lossy());
    let b_url = format!("sqlite:{}?mode=ro", b.to_string_lossy());

    let a_conn = Database::connect(&a_url).await.context("connect a")?;
    let b_conn = Database::connect(&b_url).await.context("connect b")?;

    let mut a_bundles = db::get_bundles(&a_conn).await.context("bundles a")?;
    let mut b_bundles = db::get_bundles(&b_conn).await.context("bundles b")?;
    a_bundles.sort();
    b_bundles.sort();
    assert_eq!(a_bundles, b_bundles, "bundles differ");

    let mut a_files = db::get_files_with_hash(&a_conn).await.context("files a")?;
    let mut b_files = db::get_files_with_hash(&b_conn).await.context("files b")?;
    a_files.sort();
    b_files.sort();
    assert_eq!(a_files, b_files, "files differ");

    let a_ver = db::get_version(&a_conn).await?;
    let b_ver = db::get_version(&b_conn).await?;
    assert_eq!(a_ver, b_ver, "version differs");

    Ok(())
}

#[tokio::test]
pub async fn self_diff_produces_no_updates() -> Result<()> {
    // Build two databases from the same local index.bin
    let index = test_asset_path("3.26.0.11.index.bin");
    assert!(index.exists(), "Missing test index.bin: {:?}", index);

    let url = "https://patch.poecdn.com/3.26.0.11/";

    let out_a = unique_temp_dir("selfdiff_a");
    let out_b = unique_temp_dir("selfdiff_b");
    fs::create_dir_all(&out_a)?;
    fs::create_dir_all(&out_b)?;

    run_offline_from_index(url, out_a.to_str().unwrap(), &index)
        .await
        .context("build db A from index")?;
    run_offline_from_index(url, out_b.to_str().unwrap(), &index)
        .await
        .context("build db B from index")?;

    let db_a = out_a.join("bundle_index.sqlite");
    let db_b = out_b.join("bundle_index.sqlite");

    // Extract versions
    let conn_a = Database::connect(&format!("sqlite:{}?mode=ro", db_a.display())).await?;
    let conn_b = Database::connect(&format!("sqlite:{}?mode=ro", db_b.display())).await?;
    let ver_a = db::get_version(&conn_a).await?;
    let ver_b = db::get_version(&conn_b).await?;
    let from_ver = db::extract_version_from_url(&ver_a);
    let to_ver = db::extract_version_from_url(&ver_b);
    assert_eq!(from_ver, to_ver, "Self-diff should have same version");

    // Generate differential SQL using identical DBs
    let out_dir = unique_temp_dir("selfdiff_sql");
    println!("testing in {}", out_dir.display());
    fs::create_dir_all(&out_dir)?;
    let update_sql_path = out_dir.join(format!("update-{}-to-{}.sql", from_ver, to_ver));

    db::generate_differential_update(&db_a, &db_b, &update_sql_path, &from_ver, &to_ver)
        .await
        .context("generate self-diff update sql")?;

    // Read and check contents for spurious operations
    // Converting to lowercase for case-insensitive comparison
    let sql = fs::read_to_string(&update_sql_path)?.to_lowercase();

    // Must contain header, version update
    assert!(sql.contains("differential update from"));
    assert!(sql.contains("update version set url"));

    // Must NOT contain any data-changing statements beyond version update
    let forbidden = [
        "insert into bundles",
        "delete from bundles",
        "insert into dirs",
        "delete from dirs",
        "insert into files",
        "delete from files",
        " on conflict ",
        // D1 update is transactional; script should not manage transactions itself
        "begin transaction",
        "commit",
    ];
    for needle in &forbidden {
        assert!(
            !sql.contains(needle),
            "Self-diff emitted unexpected statement: {}",
            needle
        );
    }

    // Apply the SQL to a copy of db_a and verify the DB remains identical to db_b
    let updated = out_dir.join("updated.sqlite");
    fs::copy(&db_a, &updated)?;
    apply_sql_file(&updated, &sql)
        .await
        .context("apply self-diff sql to copy")?;

    compare_databases(&updated, &db_b)
        .await
        .context("self-diff updated DB should equal original DB")?;

    Ok(())
}
