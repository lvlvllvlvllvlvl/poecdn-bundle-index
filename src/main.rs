use poecdn_bundle_index::run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap();
    let out_dir = args.next().unwrap();

    run(addr.as_str(), out_dir.as_str()).await?;

    Ok(())
}
