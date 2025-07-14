use anyhow::Error;
use base64::Engine;
use std::collections::HashSet;
use std::fs;
use std::io::BufWriter;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::Path;

mod bundle;
mod db;
mod entity;
mod models;
mod sql;
#[cfg(test)]
mod test;
mod utils;

use bundle::process_bundle;
use models::Urls;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap();
    let out_dir = args.next().unwrap();

    run(addr.as_str(), out_dir.as_str()).await?;

    Ok(())
}

async fn run(addr: &str, out_dir: &str) -> Result<(), Error> {
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

    let _ = fs::remove_dir_all(out_dir);
    fs::create_dir_all(out_dir)?;

    let raw = base64::prelude::BASE64_STANDARD_NO_PAD.encode(&buf[..read]);
    let urls_json = Path::new(out_dir).join("urls.json");

    let writer = BufWriter::new(fs::File::create(&urls_json)?);
    serde_json::to_writer_pretty(writer, &Urls { raw, urls })?;

    for v in &uniq_urls {
        process_bundle(v, out_dir).await?;
    }

    Ok(())
}
