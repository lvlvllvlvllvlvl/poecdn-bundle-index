use base64::Engine;
use sanitize_filename::Options;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::prelude::*;
use std::io::SeekFrom::Current;
use std::io::{BufReader, BufWriter, Cursor};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use url::Url;

fn main() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect("patch.pathofexile.com:12995")?;

    stream.write_all(&[1, 6])?;
    let mut buf = [0; 1000];
    let read = stream.read(&mut buf)?;
    println!("read {} bytes", read);
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

    let raw = base64::prelude::BASE64_STANDARD_NO_PAD.encode(&buf[..read]);
    if let Ok(f) = fs::File::open("output/urls.json") {
        let prev: Urls = serde_json::from_reader(f)?;
        if raw == prev.raw {
            return Ok(());
        } else {
            let _ = fs::remove_dir_all("output");
        }
    }
    fs::create_dir_all("output")?;

    let writer = BufWriter::new(fs::File::create(Path::new("./output/urls.json"))?);
    serde_json::to_writer_pretty(writer, &Urls { raw, urls })?;

    for v in &uniq_urls {
        let base = Url::parse(v)?;
        let dir = [
            "output",
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
        let count = read_u32(cur)?;
        let mut bundle_names = Vec::with_capacity(count);
        let mut bundle_sizes = Vec::with_capacity(count);
        for _ in 0..count {
            let name_len = read_u32(cur)?;
            let start = cur.position() as usize;
            let end = start + name_len;
            let name = std::str::from_utf8(&index_bundle[start..end])?;
            cur.seek(Current(name_len as i64))?;
            let bundle_size = read_u32(cur)?;
            bundle_names.push(name);
            bundle_sizes.push(bundle_size);
        }

        let mut files = HashMap::new();
        for _ in 0..read_u32(cur)? {
            files.insert(
                // hash
                read_u64(cur)?,
                // bundle index, file offset, file size
                (read_u32(cur)?, read_u32(cur)?, read_u32(cur)?),
            );
        }
        let path_rep_count = read_u32(cur)? as i64;
        cur.seek(Current(path_rep_count * 20))?;

        let path_bundle = decompress(cur)?;
        let paths = decode_paths(path_bundle.as_slice())?;
        let mut file_data = BTreeMap::new();
        for filename in paths.iter() {
            let hash = murmurhash64::murmur_hash64a(filename.as_bytes(), 0x1337b33f) as usize;
            if let Some(&(bundle_index, offset, size)) = files.get(&hash) {
                let range =
                    if offset == 0 && bundle_sizes.get(bundle_index).is_some_and(|&s| s == size) {
                        None
                    } else {
                        Some((offset, size))
                    };

                let bundle = bundle_names[bundle_index];
                let file = File { bundle, range };
                let (dir, name) = filename.rsplit_once('/').unwrap_or(("", filename));
                file_data
                    .entry(dir)
                    .or_insert_with(BTreeMap::new)
                    .insert(name, file);
            } else {
                eprintln!("No file found for hash {} of {}", hash, filename)
            }
        }

        let mut file_writer = csv::Writer::from_path(dir.join("files.csv"))?;
        file_writer.serialize(["file", "bundle", "offset", "size"])?;
        let mut prev_dir = "";
        for (dir, data) in file_data {
            let mut pb = if let Some(d) = pathdiff::diff_paths(dir, prev_dir)
                .as_ref()
                .and_then(|d| d.to_str())
                .and_then(|d| (d.len() < dir.len()).then_some(d))
            {
                PathBuf::from(d)
            } else {
                let mut buf = PathBuf::from("/");
                for seg in dir.split('/') {
                    buf.push(seg);
                }
                buf
            };
            prev_dir = dir;
            for (file, data) in data {
                pb.push(file);
                file_writer.serialize((
                    &pb,
                    data.bundle,
                    data.range.map(|v| v.0),
                    data.range.map(|v| v.1),
                ))?;
                pb.clear();
            }
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Urls {
    raw: String,
    urls: Vec<String>,
}

#[derive(Serialize)]
struct File<'a> {
    bundle: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<(usize, usize)>,
}

fn decompress<T: Read>(f: &mut T) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0; 20];
    // uncompressed size u32, payload size u32, header size u32, first file u32, unknown u32
    f.read_exact(buf.as_mut_slice())?;
    let uncompressed_size = read_u64(f)?;
    // payload size
    read_u64(f)?;
    let block_count = read_u32(f)?;
    // granularity u32,
    let granularity = read_u32(f)?;
    println!(
        "uncompressed size: {}, block count: {}, granularity: {}",
        uncompressed_size, block_count, granularity
    );
    buf.reserve(uncompressed_size - 20);
    // unknown [u32; 4]
    buf.resize(16, 0);
    f.read_exact(buf.as_mut_slice())?;
    // block sizes [u32; block_count]
    buf.resize(4 * block_count, 0);
    f.read_exact(buf.as_mut_slice())?;
    buf.resize(uncompressed_size, 0);
    let mut ooz = oozextract::Extractor::new(f);
    for i in 0..block_count {
        ooz.read(&mut buf[i * granularity..uncompressed_size.min((i + 1) * granularity)])?;
    }
    println!("Decompressed {} bytes", buf.len());
    Ok(buf)
}
fn decode_paths(data: &[u8]) -> anyhow::Result<Vec<String>> {
    let mut bases: Vec<String> = Vec::new();
    let mut results = Vec::new();
    let mut base_phase = false;
    let r = &mut Cursor::new(data);
    let fragment = &mut Vec::new();
    while r.position() < data.len() as u64 {
        let cmd = read_u32(r)?;
        if cmd == 0 {
            base_phase = !base_phase;
            if base_phase {
                bases.clear();
            }
        } else {
            fragment.clear();
            r.read_until(b'\0', fragment)?;
            let path = std::str::from_utf8(fragment)?.trim_end_matches('\0');
            let mut full;
            if cmd <= bases.len() {
                full = bases[cmd - 1].clone();
                full.push_str(path);
            } else {
                full = path.to_string();
            }
            if base_phase {
                bases.push(full);
            } else {
                results.push(full);
            }
        }
    }
    Ok(results)
}

fn read_u32<T: Read>(cur: &mut T) -> anyhow::Result<usize> {
    let mut bytes = [0; 4];
    cur.read_exact(&mut bytes[..])?;
    Ok(u32::from_le_bytes(bytes) as usize)
}

fn read_u64<T: Read>(cur: &mut T) -> anyhow::Result<usize> {
    let mut bytes = [0; 8];
    cur.read_exact(&mut bytes[..])?;
    Ok(u64::from_le_bytes(bytes) as usize)
}
