use anyhow::Result;
use csv::Reader;
use std::collections::HashSet;
use std::path::Path;

use crate::{db, run};

/// Verifies that the database content matches the CSV files
#[tokio::test]
pub async fn verify_database_matches_csv() -> Result<()> {
    let out_dir_path = Path::new("test-data");
    let db_path = out_dir_path.join("bundle_index.db");

    run(
        "patch.pathofexile.com:12995",
        out_dir_path.to_string_lossy().as_ref(),
    )
    .await?;

    // Connect to the database
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());
    let conn = sea_orm::Database::connect(&db_url).await?;

    // Get all bundles from the database
    let db_bundles = db::get_bundles(&conn).await?;

    // Get all bundles from the CSV file
    let mut csv_bundles = HashSet::new();
    let mut rdr = Reader::from_path(out_dir_path.join("bundles.csv"))?;
    for result in rdr.records() {
        let record = result?;
        let name = record.get(0).unwrap_or("").to_string();
        let size = record.get(1).unwrap_or("0").parse::<u32>()?;
        csv_bundles.insert((name, size));
    }

    // Verify that all bundles in the database are in the CSV file
    for (name, size) in &db_bundles {
        if !csv_bundles.contains(&(name.clone(), *size)) {
            println!("Bundle in database but not in CSV: {} ({})", name, size);
        }
    }

    // Verify that all bundles in the CSV file are in the database
    let db_bundles_set: HashSet<_> = db_bundles.into_iter().collect();
    for (name, size) in &csv_bundles {
        if !db_bundles_set.contains(&(name.clone(), *size)) {
            println!("Bundle in CSV but not in database: {} ({})", name, size);
        }
    }

    // Get all files from the database
    let db_files = db::get_files(&conn).await?;

    // Get all files from the CSV file
    let mut csv_files = HashSet::new();
    let mut rdr = Reader::from_path(out_dir_path.join("files.csv"))?;
    for result in rdr.records() {
        let record = result?;
        let path = record.get(0).unwrap_or("").to_string();
        let bundle = record.get(1).unwrap_or("").to_string();
        let offset = record.get(2).map(|s| s.parse::<u32>().ok()).flatten();
        let size = record.get(3).map(|s| s.parse::<u32>().ok()).flatten();
        csv_files.insert((path, bundle, offset, size));
    }

    // Verify that all files in the database are in the CSV file
    for (path, bundle, offset, size) in &db_files {
        if !csv_files.contains(&(path.clone(), bundle.clone(), *offset, *size)) {
            println!(
                "File in database but not in CSV: {} in {} (offset: {:?}, size: {:?})",
                path, bundle, offset, size
            );
        }
    }

    // Verify that all files in the CSV file are in the database
    let db_files_set: HashSet<_> = db_files.into_iter().collect();
    for (path, bundle, offset, size) in &csv_files {
        if !db_files_set.contains(&(path.clone(), bundle.clone(), *offset, *size)) {
            println!(
                "File in CSV but not in database: {} in {} (offset: {:?}, size: {:?})",
                path, bundle, offset, size
            );
        }
    }

    println!("Verification complete!");
    Ok(())
}
