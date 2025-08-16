use poecdn_bundle_index::run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let game_version = args.next().unwrap_or_else(|| {
        eprintln!("Error: Missing argument. Please specify 'poe1' or 'poe2'.");
        std::process::exit(1);
    });

    match game_version.as_str() {
        "poe1" => {
            run("patch.pathofexile.com:12995", "output/poe1").await?;
        }
        "poe2" => {
            run("patch.pathofexile2.com:13060", "output/poe2").await?;
        }
        _ => {
            eprintln!("Error: Invalid argument. Please specify 'poe1' or 'poe2'.");
            std::process::exit(1);
        }
    }

    Ok(())
}
