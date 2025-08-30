use anyhow::Result;
use itertools::Itertools;
use sanitize_filename::Options;
use sea_orm::{
    ActiveValue, ConnectionTrait, DbConn, EntityTrait, QueryTrait, Statement, TransactionTrait,
};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::prelude::*;
use std::io::{BufReader, Cursor};
use std::path::{Component, Path, PathBuf};
use url::Url;

use crate::db;
use crate::entity::prelude::*;
use crate::entity::{bundles, dirs, files, version};
use crate::models::{Dir, File};
use crate::utils::{add_dir, decode_paths, decompress, read_u32, read_u64};
use sea_query::{Query, SqliteQueryBuilder};

const SQLITE_MAX_VARIABLE_NUMBER: usize = 999;

/// Prepares the output directory structure based on the URL
fn prepare_output_directory(url_str: &str, out_dir: &str) -> Result<(PathBuf, PathBuf)> {
    let base = Url::parse(url_str)?;
    let dir = [
        out_dir,
        base.domain().unwrap_or(""),
        base.path().trim_start_matches('/'),
    ]
    .iter()
    .collect::<PathBuf>()
    .components()
    .filter_map(|c| {
        if let Component::Normal(f) = c {
            f.to_str().map(|s| {
                sanitize_filename::sanitize_with_options(
                    s,
                    Options {
                        windows: true,
                        truncate: true,
                        replacement: "",
                    },
                )
            })
        } else {
            None
        }
    })
    .collect::<PathBuf>();
    fs::create_dir_all(&dir)?;
    let out_dir_path = Path::new(out_dir).to_path_buf();

    Ok((dir, out_dir_path))
}

/// Downloads and decompresses the bundle index file
pub(crate) async fn download_and_decompress_bundle(base_url: &Url) -> Result<Vec<u8>> {
    let url = base_url.join("Bundles2/_.index.bin")?;
    // Removed logging of each URL to reduce output
    let url_str = url.to_string(); // Store URL as string for potential error logging
    let response = reqwest::get(url).await?;
    // Only log errors, not successful responses
    if !response.status().is_success() {
        println!(
            "Error downloading {}: status {}",
            url_str,
            response.status()
        );
        assert!(response.status().is_success());
    }

    decompress(&mut BufReader::new(response.bytes().await?.as_ref()))
}

/// Parses bundle names and sizes from the index bundle
fn parse_bundle_metadata<'a>(
    index_bundle: &'a [u8],
    cursor: &mut Cursor<&'a Vec<u8>>,
) -> Result<(Vec<&'a str>, Vec<u32>)> {
    let count = read_u32(cursor)? as usize;
    let mut bundle_names = Vec::with_capacity(count);
    let mut bundle_sizes = Vec::with_capacity(count);

    for _ in 0..count {
        let name_len = read_u32(cursor)? as usize;
        let start = cursor.position() as usize;
        let end = start + name_len;
        let name = std::str::from_utf8(&index_bundle[start..end])?;
        cursor.seek(std::io::SeekFrom::Current(name_len as i64))?;
        let bundle_size = read_u32(cursor)?;
        bundle_names.push(name);
        bundle_sizes.push(bundle_size);
    }

    Ok((bundle_names, bundle_sizes))
}

/// Extracts file hashes and their associated bundle information
fn extract_file_hashes(cursor: &mut Cursor<&Vec<u8>>) -> Result<BTreeMap<u64, (u32, u32, u32)>> {
    let mut files = BTreeMap::new();

    for _ in 0..read_u32(cursor)? {
        files.insert(
            // hash
            read_u64(cursor)? as u64,
            // bundle index, file offset, file size
            (read_u32(cursor)?, read_u32(cursor)?, read_u32(cursor)?),
        );
    }

    let path_rep_count = read_u32(cursor)? as i64;
    cursor.seek(std::io::SeekFrom::Current(path_rep_count * 20))?;

    Ok(files)
}

/// Generates CSV files for files and bundles
fn generate_csv_files(
    paths: &[String],
    files_map: &BTreeMap<u64, (u32, u32, u32)>,
    bundle_names: &[&str],
    bundle_sizes: &[u32],
    dir: &Path,
) -> Result<()> {
    // Generate files.csv (single file with all entries)
    let mut file_writer = csv::Writer::from_path(dir.join("files.csv"))?;
    file_writer.serialize(["hash", "file", "bundle", "offset", "size"])?;

    // Deduplicate by file hash to mirror database unique constraint on files.hash
    let mut seen_hashes: HashMap<u64, &String> = HashMap::new();

    for filename in paths.iter() {
        let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);
        if let Some(prev) = seen_hashes.insert(hash, filename) {
            assert_eq!(prev, filename);
            continue;
        }
        if let Some(&(bundle_index, offset, size)) = files_map.get(&hash) {
            let bundle = bundle_names[bundle_index as usize];
            let range = if offset == 0
                && bundle_sizes
                    .get(bundle_index as usize)
                    .is_some_and(|&s| s == size)
            {
                None
            } else {
                Some((offset, size))
            };

            file_writer.serialize((
                hash,
                filename,
                bundle,
                range.map(|v| v.0),
                range.map(|v| v.1),
            ))?;
        }
    }

    // Generate bundles.csv
    let mut file_writer = csv::Writer::from_path(dir.join("bundles.csv"))?;
    file_writer.serialize(["bundle", "size"])?;
    for (&bundle_name, &bundle_size) in bundle_names.iter().zip(bundle_sizes) {
        file_writer.serialize((bundle_name, bundle_size))?;
    }

    Ok(())
}

/// Generates SQL files for bundles, directories, and version
async fn generate_sql_files<'a>(
    bundle_names: &'a [&'a str],
    bundle_sizes: &'a [u32],
    paths: &'a [String],
    files_map: &'a BTreeMap<u64, (u32, u32, u32)>,
    all_dirs: &BTreeMap<&'a str, Dir>,
    out_dir: &Path,
    url_str: &str,
    conn: &DbConn,
) -> Result<()> {
    // Begin a transaction for better performance
    let tx = conn.begin().await?;
    tx.execute(Statement::from_string(
        conn.get_database_backend(),
        "PRAGMA defer_foreign_keys = on;",
    ))
    .await?;

    // Generate bundles.sql
    let mut bundles_writer = fs::File::create(out_dir.join("bundles.sql"))?;

    for chunk in &bundle_names
        .iter()
        .enumerate()
        .chunks(SQLITE_MAX_VARIABLE_NUMBER / 3)
    // each ActiveValue::Set counts as a variable
    {
        let insert = Bundles::insert_many(chunk.map(|(index, name)| bundles::ActiveModel {
            id: ActiveValue::set(index as u32),
            name: ActiveValue::Set(name.to_string()),
            size: ActiveValue::Set(bundle_sizes[index]),
        }));
        let stmt = insert.build(conn.get_database_backend());
        // Write a copy of the statement to the sql file
        writeln!(bundles_writer, "{stmt}")?;
        // Run the same statement on the db
        insert.exec(&tx).await?;
    }

    // Generate dirs.sql
    let mut dirs_writer = fs::File::create(out_dir.join("dirs.sql"))?;

    for chunk in &all_dirs.iter().chunks(SQLITE_MAX_VARIABLE_NUMBER / 3) {
        let insert = Dirs::insert_many(chunk.map(|(name, dir)| dirs::ActiveModel {
            id: ActiveValue::Set(dir.id),
            name: ActiveValue::Set(name.to_string()),
            parent: ActiveValue::Set(dir.parent),
        }));
        let stmt = insert.build(conn.get_database_backend());
        writeln!(dirs_writer, "{stmt}")?;
        insert.exec(&tx).await?;
    }

    // Generate files.sql
    let mut files_writer = fs::File::create(out_dir.join("files.sql"))?;

    for chunk in &paths.iter().chunks(SQLITE_MAX_VARIABLE_NUMBER / 6) {
        let insert = Files::insert_many(chunk.map(|filename| {
            let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);
            let &(bundle_index, offset, size) = files_map.get(&hash).unwrap();
            let (dir_str, name_str) = filename.rsplit_once('/').unwrap_or(("", filename));
            let dir_id = all_dirs.get(dir_str).map(|d| d.id).unwrap_or(0);

            files::ActiveModel {
                hash: ActiveValue::Set(hash as i64),
                dir: ActiveValue::Set(dir_id),
                name: ActiveValue::Set(name_str.to_string()),
                bundle: ActiveValue::Set(bundle_index),
                offset: ActiveValue::Set(offset),
                size: ActiveValue::Set(size),
            }
        }));
        let stmt = insert.build(conn.get_database_backend());
        writeln!(files_writer, "{stmt}")?;
        insert.exec(&tx).await?;
    }

    // Generate version.sql
    let version_sql = Query::insert()
        .into_table(Version)
        .columns([version::Column::Id, version::Column::Url])
        .values([0.into(), url_str.into()])?
        .to_string(SqliteQueryBuilder);
    let mut version_writer = fs::File::create(out_dir.join("version.sql"))?;
    writeln!(version_writer, "{version_sql};")?;

    // Insert version into the database
    db::insert_version(&tx, url_str).await?;

    // Commit the transaction
    tx.commit().await?;

    Ok(())
}

pub async fn process_bundle(url_str: &str, out_dir: &str, index_bundle: Vec<u8>) -> Result<()> {
    process_bundle_bytes(index_bundle, url_str, out_dir).await
}

pub async fn process_bundle_bytes(
    index_bundle: Vec<u8>,
    url_str: &str,
    out_dir: &str,
) -> Result<()> {
    // Prepare output directory
    let (dir, out_dir_path) = prepare_output_directory(url_str, out_dir)?;

    // Create database connection
    let conn = db::create_database(&out_dir_path).await?;

    // Parse bundle metadata
    let cursor = &mut Cursor::new(&index_bundle);
    let (bundle_names, bundle_sizes) = parse_bundle_metadata(&index_bundle, cursor)?;

    // Extract file hashes
    let files = extract_file_hashes(cursor)?;

    // Process path bundle
    let path_bundle = decompress(cursor)?;
    let paths = decode_paths(path_bundle.as_slice())?;

    // First, collect all directories and prepare file data without inserting into the database
    let mut file_data = BTreeMap::new();
    let mut all_dirs = BTreeMap::new();

    for filename in paths.iter() {
        // Always register the directory for every decoded path
        let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));
        let _dir_id = add_dir(dir, &mut all_dirs);

        let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);

        if let Some(&(bundle_index, offset, size)) = files.get(&hash) {
            let range = if offset == 0
                && bundle_sizes
                    .get(bundle_index as usize)
                    .is_some_and(|&s| s == size)
            {
                None
            } else {
                Some((offset, size))
            };

            let bundle = bundle_names[bundle_index as usize];
            let file = File { bundle, range };

            file_data
                .entry(dir)
                .or_insert_with(BTreeMap::new)
                .insert(name, file);
        } else {
            println!("File not found in index bundle: {filename}");
        }
    }

    // Generate CSV files for reference
    generate_csv_files(&paths, &files, &bundle_names, &bundle_sizes, &dir)?;

    // Generate SQL files for initializing the database
    generate_sql_files(
        &bundle_names,
        &bundle_sizes,
        &paths,
        &files,
        &all_dirs,
        &out_dir_path,
        url_str,
        &conn,
    )
    .await?;

    Ok(())
}

/// Processes a bundle using a pre-downloaded local index.bin file (offline mode)
pub async fn process_bundle_from_local_index(
    url_str: &str,
    out_dir: &str,
    index_path: &Path,
) -> Result<()> {
    // Read and decompress local index.bin
    let file = fs::File::open(index_path)?;
    let mut reader = BufReader::new(file);
    let index_bundle = decompress(&mut reader)?;

    process_bundle_bytes(index_bundle, url_str, out_dir).await
}
