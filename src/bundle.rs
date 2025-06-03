use std::collections::{BTreeMap, HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::fs;
use std::io::{BufReader, BufWriter, Cursor};
use std::path::{Path, PathBuf, Component};
use std::io::prelude::*;
use anyhow::Result;
use url::Url;
use sanitize_filename::Options;
use base64::Engine;
use murmurhash64;

use crate::models::{Urls, File, Dir};
use crate::utils::{decompress, decode_paths, read_u32, read_u64, add_dir, skip};
use crate::sql::{insert_files, insert_bundles, insert_dirs};
use crate::entity::{bundles, dirs, version};
use crate::entity::prelude::*;
use sea_query::{Query, SqliteQueryBuilder};

pub fn process_bundle(url_str: &str, out_dir: &str) -> Result<()> {
    const SQL_LINES: i32 = 200;

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
    let out_dir = Path::new(out_dir);

    let url = base.join("Bundles2/_.index.bin")?;
    println!("download url: {}", url);
    let response = reqwest::blocking::get(url)?;
    println!(
        "status: {}, length: {:?}",
        response.status(),
        response.content_length()
    );
    assert!(response.status().is_success());
    let index_bundle = decompress(&mut BufReader::new(response))?;
    let cur = &mut Cursor::new(&index_bundle);
    let count = read_u32(cur)? as usize;
    let mut bundle_names = Vec::with_capacity(count);
    let mut bundle_sizes = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = read_u32(cur)? as usize;
        let start = cur.position() as usize;
        let end = start + name_len;
        let name = std::str::from_utf8(&index_bundle[start..end])?;
        cur.seek(std::io::SeekFrom::Current(name_len as i64))?;
        let bundle_size = read_u32(cur)?;
        bundle_names.push(name);
        bundle_sizes.push(bundle_size);
    }

    let mut files = BTreeMap::new();
    for _ in 0..read_u32(cur)? {
        files.insert(
            // hash
            read_u64(cur)? as u64,
            // bundle index, file offset, file size
            (read_u32(cur)?, read_u32(cur)?, read_u32(cur)?),
        );
    }
    let path_rep_count = read_u32(cur)? as i64;
    cur.seek(std::io::SeekFrom::Current(path_rep_count * 20))?;

    let path_bundle = decompress(cur)?;
    let paths = decode_paths(path_bundle.as_slice())?;
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

            if !skip(dir) {
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
            }

            file_data
                .entry(dir)
                .or_insert_with(BTreeMap::new)
                .insert(name, file);
        } else {
            eprintln!("No file found for hash {} of {}", hash, filename)
        }
    }

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

    file_writer = csv::Writer::from_path(dir.join("bundles.csv"))?;
    file_writer.serialize(["bundle", "size"])?;
    for (&bundle_name, &bundle_size) in bundle_names.iter().zip(&bundle_sizes) {
        file_writer.serialize((bundle_name, bundle_size))?;
    }

    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
    let mut sql_writer = fs::File::create(out_dir.join("bundles.sql"))?;
    let mut sql = insert_bundles();
    for (bundle_index, &bundle_name) in bundle_names.iter().enumerate() {
        sql.values([
            (bundle_index as u64).into(),
            bundle_name.into(),
            bundle_sizes[bundle_index].into(),
        ])?;
        if sql_line % SQL_LINES == 0 && sql_line > 0 {
            writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
            sql = insert_bundles();
        }
        sql_line += 1;
    }
    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;

    let mut sql_writer = fs::File::create(out_dir.join("dirs.sql"))?;
    let mut sql = insert_dirs();
    for (name, Dir { id, parent }) in all_dirs {
        sql.values([id.into(), name.into(), parent.into()])?;
        if sql_line % SQL_LINES == 0 && sql_line > 0 {
            writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;
            sql = insert_dirs();
        }
        sql_line += 1;
    }
    writeln!(sql_writer, "{};", sql.to_string(SqliteQueryBuilder))?;

    let sql = Query::insert()
        .into_table(Version)
        .columns([version::Column::Id, version::Column::Url])
        .values([0.into(), url_str.into()])?.to_string(SqliteQueryBuilder);
    let mut sql_writer = fs::File::create(out_dir.join("version.sql"))?;
    writeln!(sql_writer, "{};", sql)?;

    Ok(())
}
