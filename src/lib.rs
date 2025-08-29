use anyhow::{Context, Error};
use base64::Engine;
use sea_orm::Database;
use std::collections::HashSet;
use std::fs;
use std::io::BufWriter;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use url::Url;

mod bundle;
pub mod db;
mod entity;
mod models;
mod utils;

use bundle::{process_bundle, process_bundle_from_local_index};
use db::{
    check_d1_version, download_previous_database, extract_version_from_url,
    generate_differential_update, get_version,
};
use models::Urls;

/// Determine the game type from the address
fn get_game_type(addr: &str) -> &'static str {
    if addr.contains("12995") || addr.contains("pathofexile.com") {
        "poe1"
    } else {
        "poe2"
    }
}

pub async fn run(addr: &str, out_dir: &str) -> Result<(), Error> {
    let game_type = get_game_type(addr);
    println!("Processing {} data", game_type);

    // Create output directory if it doesn't exist
    let _ = fs::remove_dir_all(out_dir);
    fs::create_dir_all(out_dir)?;

    let (uniq_urls, current_version) = get_cdn_urls(addr, out_dir)?;

    // Process bundles
    for v in &uniq_urls {
        // Download and decompress bundle
        let base = Url::parse(v)?;
        let index_bundle = bundle::download_and_decompress_bundle(&base).await?;
        process_bundle(v, out_dir, index_bundle).await?;
    }

    diff_db(out_dir, game_type, current_version).await?;

    Ok(())
}

async fn diff_db(out_dir: &str, game_type: &str, current_version: String) -> Result<(), Error> {
    // Generate differential update if appropriate
    let out_dir_path = PathBuf::from(out_dir);
    let current_db_path = out_dir_path.join("bundle_index.sqlite");

    // Create the database (this is needed for the differential update)
    let _conn = db::create_database(&out_dir_path).await?;

    println!("Current version: {}", current_version);

    // Check if we should generate a differential update
    let prev_db_path = out_dir_path.join("previous_bundle_index.sqlite");

    // Try to download the previous database
    if let Err(e) = download_previous_database(game_type, &prev_db_path).await {
        println!("Could not download previous database: {}", e);
        println!("Skipping differential update generation");
        return Ok(());
    }

    // Connect to the previous database
    let prev_db_url = format!("sqlite:{}?mode=ro", prev_db_path.to_string_lossy());
    let prev_conn = Database::connect(&prev_db_url)
        .await
        .context("Failed to connect to previous database")?;

    // Try to get the version from the previous database
    let prev_version = match get_version(&prev_conn).await {
        Ok(prev_version_url) => extract_version_from_url(&prev_version_url),
        Err(e) => {
            println!("Could not get version from previous database: {}", e);
            println!("Skipping differential update generation");
            return Ok(());
        }
    };

    println!("Previous version: {}", prev_version);

    // If versions are different, generate a differential update
    if current_version != prev_version {
        let update_sql_path = out_dir_path.join(format!(
            "update-{}-to-{}.sql",
            prev_version, current_version
        ));

        println!(
            "Generating differential update from {} to {}",
            prev_version, current_version
        );
        generate_differential_update(
            &prev_db_path,
            &current_db_path,
            &update_sql_path,
            &prev_version,
            &current_version,
        )
        .await?;

        // Check if we should use this update in D1
        if let Some(d1_version) = check_d1_version(game_type).await? {
            if d1_version == prev_version {
                println!("D1 version matches previous version, update script can be used");

                // Create a marker file to indicate that the update script should be used
                let marker_path = out_dir_path.join("use_update_script");
                fs::write(
                    marker_path,
                    format!("{}\n{}", prev_version, current_version),
                )?;
            } else {
                println!(
                    "D1 version ({}) does not match previous version ({}), full rebuild required",
                    d1_version, prev_version
                );
            }
        } else {
            println!("Could not determine D1 version, assuming full rebuild required");
        }
    } else {
        println!("Current version matches previous version, no update needed");
    }
    Ok(())
}

fn get_cdn_urls(addr: &str, out_dir: &str) -> Result<(HashSet<String>, String), Error> {
    println!("Connecting to {}", addr);

    let mut stream = TcpStream::connect(addr)?;

    stream.write_all(&[1, 7])?;
    let mut buf = [0; 1000];
    let read = stream.read(&mut buf)?;
    println!("Read {} bytes", read);
    assert!(read > 33);

    let mut urls = Vec::new();
    let mut uniq_urls = HashSet::new();
    let mut data = &buf[34..read];
    while !data.is_empty() {
        let len = data[0] as usize;
        data = &data[1..];
        if len == 0 {
            continue;
        } else if len > data.len() {
            eprintln!("len {} too big", len);
            break;
        }
        let raw = data
            .chunks(2)
            .take(len)
            .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        let url = String::from_utf16(&raw)?;
        urls.push(url.clone());
        uniq_urls.insert(url);
        data = &data[2 * len..];
    }

    // Get the version from the first URL (they should all have the same version)
    let first_url = urls
        .first()
        .ok_or_else(|| anyhow::anyhow!("No URLs found"))?;
    let current_version = extract_version_from_url(first_url);
    println!("Current version: {}", current_version);

    // Save URLs to JSON file
    let raw = base64::prelude::BASE64_STANDARD_NO_PAD.encode(&buf[..read]);
    let urls_json = Path::new(out_dir).join("urls.json");

    let writer = BufWriter::new(fs::File::create(&urls_json)?);
    serde_json::to_writer_pretty(writer, &Urls { raw, urls })?;
    Ok((uniq_urls, current_version))
}

/// Offline entry point: process a single bundle from a local index.bin without any network calls
pub async fn run_offline_from_index(
    url: &str,
    out_dir: &str,
    index_path: &Path,
) -> Result<(), Error> {
    process_bundle_from_local_index(url, out_dir, index_path)
        .await
        .map_err(|e| e.into())
}
