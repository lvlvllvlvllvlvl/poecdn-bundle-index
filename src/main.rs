use clap::{Parser, Subcommand};
use poecdn_bundle_index::{run, run_offline_from_index};
use poecdn_bundle_index::db::{generate_differential_update, get_version, extract_version_from_url};
use sea_orm::Database;
use std::path::PathBuf;
use anyhow::Context;
use poecdn_bundle_index::exporter::generate_root_index;
use std::path::Path;

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
    /// Generate differential update SQL by comparing two existing SQLite DBs
    DiffUpdate {
        /// Path to the previous SQLite database
        #[arg(long)]
        previous: PathBuf,
        /// Path to the current SQLite database
        #[arg(long)]
        current: PathBuf,
        /// Optional output path for the generated SQL file. If omitted, a file named
        /// update-<from>-to-<to>.sql will be placed next to the current database.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Poe1 { out_dir } => {
            run("patch.pathofexile.com:12995", &out_dir).await?;
            generate_root_index(Path::new("output"))?;
        }
        Commands::Poe2 { out_dir } => {
            run("patch.pathofexile2.com:13060", &out_dir).await?;
            generate_root_index(Path::new("output"))?;
        }
        Commands::Offline { url, index, out_dir } => {
            run_offline_from_index(&url, &out_dir, &index).await?;
            if let Some(parent) = Path::new(&out_dir).parent() {
                generate_root_index(parent)?;
            }
        }
        Commands::DiffUpdate { previous, current, output } => {
            // Open both databases to derive versions (for naming and validation)
            let prev_url = format!("sqlite:{}?mode=ro", previous.to_string_lossy());
            let curr_url = format!("sqlite:{}?mode=ro", current.to_string_lossy());

            let prev_conn = Database::connect(&prev_url)
                .await
                .context("Failed to connect to previous database")?;
            let curr_conn = Database::connect(&curr_url)
                .await
                .context("Failed to connect to current database")?;

            let prev_version_url = get_version(&prev_conn).await
                .context("Failed to get version from previous database")?;
            let curr_version_url = get_version(&curr_conn).await
                .context("Failed to get version from current database")?;

            let from_version = extract_version_from_url(&prev_version_url);
            let to_version = extract_version_from_url(&curr_version_url);

            let output_path = match output {
                Some(p) => p,
                None => {
                    let mut p = current.clone();
                    p.set_file_name(format!("update-{from_version}-to-{to_version}.sql"));
                    p
                }
            };

            generate_differential_update(
                &previous,
                &current,
                &output_path,
                &from_version,
                &to_version,
            )
            .await?;

            println!(
                "Differential update generated: {}",
                output_path.to_string_lossy()
            );
        }
    }

    Ok(())
}
