use base64::Engine;
use std::collections::HashSet;
use std::fs;
use std::io::prelude::*;
use std::io::BufWriter;
use std::net::TcpStream;
use std::path::Path;

mod entity;
mod models;
mod utils;
mod sql;
mod bundle;

use bundle::process_bundle;
use models::Urls;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap();
    let out_dir = args.next().unwrap();

    println!("connecting to {}", addr);
    let mut stream = TcpStream::connect(addr)?;

    stream.write_all(&[1, 7])?;
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
    let urls_json = Path::new(out_dir.as_str()).join("urls.json");
    if let Ok(f) = fs::File::open(&urls_json) {
        let prev: Urls = serde_json::from_reader(f)?;
        if raw == prev.raw {
            return Ok(());
        } else {
            let _ = fs::remove_dir_all(out_dir.as_str());
        }
    }
    fs::create_dir_all(out_dir.as_str())?;

    let writer = BufWriter::new(fs::File::create(&urls_json)?);
    serde_json::to_writer_pretty(writer, &Urls { raw, urls })?;

    for v in &uniq_urls {
        process_bundle(v, out_dir.as_str())?;
    }

    Ok(())
}
