use anyhow::Error;
use base64::Engine;
use std::collections::HashSet;
use std::fs;
use std::io::BufWriter;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::Path;
use url::Url;

mod bundle;
pub mod db;
mod entity;
pub mod exporter;
mod models;
mod utils;

use bundle::{process_bundle, process_bundle_from_local_index};
use db::extract_version_from_url;
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
    println!("Processing {game_type} data");

    // Create output directory if it doesn't exist
    let _ = fs::remove_dir_all(out_dir);
    fs::create_dir_all(out_dir)?;

    let uniq_urls = get_cdn_urls(addr, out_dir)?;

    // Process bundles
    for v in &uniq_urls {
        // Download and decompress bundle
        let base = Url::parse(v)?;
        let index_bundle = bundle::download_and_decompress_bundle(&base).await?;
        process_bundle(v, out_dir, index_bundle).await?;
    }

    Ok(())
}

fn get_cdn_urls(addr: &str, out_dir: &str) -> Result<HashSet<String>, Error> {
    println!("Connecting to {addr}");

    let mut stream = TcpStream::connect(addr)?;

    stream.write_all(&[1, 7])?;
    let mut buf = [0; 1000];
    let read = stream.read(&mut buf)?;
    println!("Read {read} bytes");
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
            eprintln!("len {len} too big");
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
    println!("Current version: {current_version}");

    // Save URLs to JSON file
    let raw = base64::prelude::BASE64_STANDARD_NO_PAD.encode(&buf[..read]);
    let urls_json = Path::new(out_dir).join("urls.json");

    let writer = BufWriter::new(fs::File::create(&urls_json)?);
    serde_json::to_writer_pretty(writer, &Urls { raw, urls })?;
    Ok(uniq_urls)
}

/// Offline entry point: process a single bundle from a local index.bin without any network calls
pub async fn run_offline_from_index(
    url: &str,
    out_dir: &str,
    index_path: &Path,
) -> Result<(), Error> {
    process_bundle_from_local_index(url, out_dir, index_path).await
}
