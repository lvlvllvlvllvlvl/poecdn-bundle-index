use anyhow::Result;
use murmurhash64;
use reqwest;
use sanitize_filename::Options;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::prelude::*;
use std::io::{BufReader, Cursor};
use std::path::{Component, Path, PathBuf};
use url::Url;

use crate::entity::prelude::*;
use crate::entity::version;
use crate::models::{Dir, File};
use crate::sql::{insert_bundles, insert_dirs, insert_files};
use crate::utils::{add_dir, decode_paths, decompress, read_u32, read_u64};
use sea_query::{Query, SqliteQueryBuilder};

const SQL_LINES: i32 = 200;

/// Prepares the output directory structure based on the URL
fn prepare_output_directory(url_str: &str, out_dir: &str) -> Result<(PathBuf, PathBuf)> {
    let base = Url::parse(url_str)?;
    let dir = [
        out_dir,
        base.domain().unwrap_or(""),
        base.path().trim_start_matches(|c| c == '/'),
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
fn download_and_decompress_bundle(base_url: &Url) -> Result<Vec<u8>> {
    let url = base_url.join("Bundles2/_.index.bin")?;
    println!("download url: {}", url);
    let response = reqwest::blocking::get(url)?;
    println!(
        "status: {}, length: {:?}",
        response.status(),
        response.content_length()
    );
    assert!(response.status().is_success());

    decompress(&mut BufReader::new(response))
}

/// Parses bundle names and sizes from the index bundle
fn parse_bundle_metadata<'a>(
    index_bundle: &'a Vec<u8>,
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
fn extract_file_hashes(
    cursor: &mut Cursor<&Vec<u8>>,
) -> Result<BTreeMap<u64, (u32, u32, u32)>> {
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

/// Processes file paths and generates file data and SQL entries
fn process_file_paths<'a>(
    paths: &'a [String],
    files: &'a BTreeMap<u64, (u32, u32, u32)>,
    bundle_names: &'a [&'a str],
    bundle_sizes: &'a [u32],
    out_dir: &'a Path,
) -> Result<(
    BTreeMap<&'a str, BTreeMap<&'a str, File<'a>>>,
    BTreeMap<&'a str, Dir>,
    i32,
)> {
    let mut file_data = BTreeMap::new();
    let mut hash_map = HashMap::new();
    let mut all_dirs = BTreeMap::new();
    let mut sql_writer = fs::File::create(out_dir.join("files.sql"))?;
    let mut sql = insert_files();
    let mut sql_line = 0;

    for filename in paths.iter() {
        let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f);
        match hash_map.entry(hash) {
            Entry::Occupied(s) => println!("hash collision {} / {}", filename, s.get()),
            Entry::Vacant(e) => {
                e.insert(filename.as_str());
            }
        }

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
            let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));

            let dir_id = add_dir(dir, &mut all_dirs);
            sql.values([
                (hash as i64).into(),
                dir_id.into(),
                name.into(),
                bundle_index.into(),
                offset.into(),
                size.into(),
            ])?;

            if sql_line % SQL_LINES == 0 && sql_line > 0 {
                writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
                sql = insert_files();
            }
            sql_line += 1;

            file_data
                .entry(dir)
                .or_insert_with(BTreeMap::new)
                .insert(name, file);
        } else {
            eprintln!("No file found for hash {} of {}", hash, filename)
        }
    }

    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;

    Ok((file_data, all_dirs, sql_line))
}

/// Generates CSV files for files and bundles
fn generate_csv_files<'a>(
    file_data: &BTreeMap<&'a str, BTreeMap<&'a str, File<'a>>>,
    bundle_names: &'a [&'a str],
    bundle_sizes: &'a [u32],
    dir: &Path,
) -> Result<()> {
    // Generate files.csv
    let mut out_file_number = 0;
    let mut in_file_number = 0;
    let mut file_writer = csv::Writer::from_path(dir.join("files.csv"))?;
    file_writer.serialize(["file", "bundle", "offset", "size"])?;

    for (cur_dir, data) in file_data {
        if in_file_number / 100000 != out_file_number {
            out_file_number = in_file_number / 100000;
            file_writer = csv::Writer::from_path(dir.join("files.csv"))?;
            file_writer.serialize(["file", "bundle", "offset", "size"])?;
        }
        for (file, data) in data {
            in_file_number += 1;
            file_writer.serialize((
                format!("{}/{}", cur_dir, file),
                data.bundle,
                data.range.map(|v| v.0),
                data.range.map(|v| v.1),
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
fn generate_sql_files<'a>(
    bundle_names: &'a [&'a str],
    bundle_sizes: &'a [u32],
    all_dirs: BTreeMap<&'a str, Dir>,
    out_dir: &Path,
    url_str: &str,
    sql_line: i32,
) -> Result<()> {
    // Generate bundles.sql
    let mut sql_writer = fs::File::create(out_dir.join("bundles.sql"))?;
    let mut sql = insert_bundles();
    let mut current_sql_line = sql_line;

    for (bundle_index, &bundle_name) in bundle_names.iter().enumerate() {
        sql.values([
            (bundle_index as u64).into(),
            bundle_name.into(),
            bundle_sizes[bundle_index].into(),
        ])?;
        if current_sql_line % SQL_LINES == 0 && current_sql_line > 0 {
            writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
            sql = insert_bundles();
        }
        current_sql_line += 1;
    }
    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;

    // Generate dirs.sql
    let mut sql_writer = fs::File::create(out_dir.join("dirs.sql"))?;
    let mut sql = insert_dirs();
    for (name, Dir { id, parent }) in all_dirs {
        sql.values([id.into(), name.into(), parent.into()])?;
        if current_sql_line % SQL_LINES == 0 && current_sql_line > 0 {
            writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
            sql = insert_dirs();
        }
        current_sql_line += 1;
    }
    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;

    // Generate version.sql
    let sql = Query::insert()
        .into_table(Version)
        .columns([version::Column::Id, version::Column::Url])
        .values([0.into(), url_str.into()])?
        .to_string(SqliteQueryBuilder);
    let mut sql_writer = fs::File::create(out_dir.join("version.sql"))?;
    writeln!(sql_writer, "{};", sql)?;

    Ok(())
}

pub fn process_bundle(url_str: &str, out_dir: &str) -> Result<()> {
    // Prepare output directory
    let (dir, out_dir_path) = prepare_output_directory(url_str, out_dir)?;

    // Download and decompress bundle
    let base = Url::parse(url_str)?;
    let index_bundle = download_and_decompress_bundle(&base)?;

    // Parse bundle metadata
    let cursor = &mut Cursor::new(&index_bundle);
    let (bundle_names, bundle_sizes) = parse_bundle_metadata(&index_bundle, cursor)?;

    // Extract file hashes
    let files = extract_file_hashes(cursor)?;

    // Process path bundle
    let path_bundle = decompress(cursor)?;
    let paths = decode_paths(path_bundle.as_slice())?;

    // Process file paths and generate file data and SQL entries
    let (file_data, all_dirs, sql_line) =
        process_file_paths(&paths, &files, &bundle_names, &bundle_sizes, &out_dir_path)?;

    // Generate CSV files
    generate_csv_files(&file_data, &bundle_names, &bundle_sizes, &dir)?;

    // Generate SQL files
    generate_sql_files(
        &bundle_names,
        &bundle_sizes,
        all_dirs,
        &out_dir_path,
        url_str,
        sql_line,
    )?;

    Ok(())
}
