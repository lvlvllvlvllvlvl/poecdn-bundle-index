use crate::models::Dir;
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::Read;
use std::io::prelude::*;

pub fn add_dir<'a>(cur_dir: &'a str, all_dirs: &mut BTreeMap<&'a str, Dir>) -> u32 {
    let parent = cur_dir.rsplit_once('/').map(|v| add_dir(v.0, all_dirs));
    let id = all_dirs.len() as u32;
    all_dirs.entry(cur_dir).or_insert(Dir { id, parent }).id
}

pub fn decompress<T: Read>(f: &mut T) -> Result<Vec<u8>> {
    let mut buf = vec![0; 20];
    // uncompressed size u32, payload size u32, header size u32, first file u32, unknown u32
    f.read_exact(buf.as_mut_slice())?;
    let uncompressed_size = read_u64(f)?;
    // payload size
    read_u64(f)?;
    let block_count = read_u32(f)? as usize;
    // granularity u32,
    let granularity = read_u32(f)? as usize;
    println!(
        "uncompressed size: {uncompressed_size}, block count: {block_count}, granularity: {granularity}"
    );
    buf.reserve(uncompressed_size - 20);
    // unknown [u32; 4]
    buf.resize(16, 0);
    f.read_exact(buf.as_mut_slice())?;
    // block sizes [u32; block_count]
    buf.resize(4 * block_count, 0);
    f.read_exact(buf.as_mut_slice())?;
    buf.resize(uncompressed_size, 0);
    let mut ooz = oozextract::Extractor::new();
    for i in 0..block_count {
        ooz.read(
            f,
            &mut buf[i * granularity..uncompressed_size.min((i + 1) * granularity)],
        )?;
    }
    println!("Decompressed {} bytes", buf.len());
    Ok(buf)
}

pub fn decode_paths(data: &[u8]) -> Result<Vec<String>> {
    let mut bases: Vec<String> = Vec::new();
    let mut results = Vec::new();
    let mut base_phase = false;
    let r = &mut std::io::Cursor::new(data);
    let fragment = &mut Vec::new();
    while r.position() < data.len() as u64 {
        let cmd = read_u32(r)? as usize;
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
            } else if !skip(full.as_str()) {
                results.push(full);
            }
        }
    }
    Ok(results)
}

fn skip(path: &str) -> bool {
    path.split_once('/')
        .is_some_and(|(root, _)| root.contains("cache"))
}

pub fn read_u32<T: Read>(cur: &mut T) -> Result<u32> {
    let mut bytes = [0; 4];
    cur.read_exact(&mut bytes[..])?;
    Ok(u32::from_le_bytes(bytes))
}

pub fn read_u64<T: Read>(cur: &mut T) -> Result<usize> {
    let mut bytes = [0; 8];
    cur.read_exact(&mut bytes[..])?;
    Ok(u64::from_le_bytes(bytes) as usize)
}
