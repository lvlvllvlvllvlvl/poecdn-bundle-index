use clap::{Parser, Subcommand};
use poecdn_bundle_index::{run, run_offline_from_index};

#[derive(Parser)]
#[command(name = "poecdn-bundle-index", version, about = "Path of Exile CDN Bundle Index tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Process Path of Exile 1 data from the CDN
    Poe1 {
        /// Output directory (defaults to output/poe1)
        #[arg(long, default_value = "output/poe1")]
        out_dir: String,
    },
    /// Process Path of Exile 2 data from the CDN
    Poe2 {
        /// Output directory (defaults to output/poe2)
        #[arg(long, default_value = "output/poe2")]
        out_dir: String,
    },
    /// Offline mode: process a single bundle from a local index.bin without network calls
    Offline {
        /// Base URL prefix for the bundle paths (e.g. https://patch.poecdn.com/3.26.0.11/)
        #[arg(long)]
        url: String,
        /// Path to the local index.bin file to parse
        #[arg(long)]
        index: std::path::PathBuf,
        /// Output directory to write results into
        #[arg(long)]
        out_dir: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Poe1 { out_dir } => {
            run("patch.pathofexile.com:12995", &out_dir).await?;
        }
        Commands::Poe2 { out_dir } => {
            run("patch.pathofexile2.com:13060", &out_dir).await?;
        }
        Commands::Offline { url, index, out_dir } => {
            run_offline_from_index(&url, &out_dir, &index).await?;
        }
    }

    Ok(())
}
