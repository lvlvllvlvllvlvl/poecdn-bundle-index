use anyhow::Result;
use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DbConn, EntityTrait, QueryOrder, Set};
use std::fs;
use std::path::Path;

use crate::entity::prelude::*;
use crate::entity::{bundles, dirs, files, version};

/// Creates a new SQLite database and initializes it with the schema
pub async fn create_database(out_dir: &Path) -> Result<DbConn> {
    let db_path = out_dir.join("bundle_index.db?mode=rwc");

    // Remove existing database if it exists
    if db_path.exists() {
        fs::remove_file(&db_path)?;
    }

    // Create a new database
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());
    let conn = Database::connect(&db_url).await?;

    // Read and execute the schema creation SQL
    let schema = fs::read_to_string(Path::new("sql/create_tables.sql"))?;
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        schema,
    ))
    .await?;

    Ok(conn)
}

/// Inserts a bundle into the database
pub async fn insert_bundle<C>(conn: &C, id: u64, name: &str, size: u32) -> Result<()> 
where
    C: sea_orm::ConnectionTrait,
{
    let bundle = bundles::ActiveModel {
        id: Set(id as i32),
        name: Set(name.to_owned()),
        size: Set(size as i32),
    };

    bundle.insert(conn).await?;

    Ok(())
}

/// Inserts a directory into the database
pub async fn insert_dir<C>(conn: &C, id: u32, name: &str, parent: Option<u32>) -> Result<()> 
where
    C: sea_orm::ConnectionTrait,
{
    let dir = dirs::ActiveModel {
        id: Set(id as i32),
        name: Set(name.to_owned()),
        parent: Set(parent.map(|p| p as i32)),
    };

    dir.insert(conn).await?;

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
    let file = files::ActiveModel {
        hash: Set(hash as i32),
        dir: Set(Some(dir as i32)),
        name: Set(name.to_owned()),
        bundle: Set(bundle as i32),
        offset: Set(offset as i32),
        size: Set(size as i32),
    };

    file.insert(conn).await?;

    Ok(())
}

/// Inserts a version into the database
pub async fn insert_version<C>(conn: &C, url: &str) -> Result<()> 
where
    C: sea_orm::ConnectionTrait,
{
    let ver = version::ActiveModel {
        id: Set(0),
        url: Set(url.to_owned()),
    };

    ver.insert(conn).await?;

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
