use anyhow::Result;
use clap::{Parser, Subcommand};
use crate::scatter_parser as sp;

/// Top-level CLI struct.
#[derive(Parser)]
#[command(name = "pawflash", about = "MTK device flashing toolkit", version)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Optional subcommand.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Force a `MediaTek` device into fastboot mode
    #[command(name = "force-fastboot")]
    ForceFastboot {
        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,
    },
    /// Inspect MTK scatter files and build flash plans
    Scatter {
        /// Scatter action subcommand.
        #[command(subcommand)]
        action: ScatterAction,
    },
}

/// Scatter sub-subcommands.
#[derive(Subcommand)]
pub enum ScatterAction {
    /// Print parsed scatter metadata
    Parse {
        /// Path to the scatter file
        scatter: std::path::PathBuf,

        /// Output full JSON of the parsed scatter
        #[arg(long)]
        full_json: bool,
    },
    /// Build and display a flash plan
    Plan {
        /// Path to the scatter file
        scatter: std::path::PathBuf,

        /// Output plan as JSON
        #[arg(long)]
        json: bool,

        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,

        /// Flash planning mode
        #[arg(long, default_value = "dry-run")]
        mode: String,

        /// Storage layout selection
        #[arg(long, default_value = "auto")]
        storage: String,

        /// Explicit partition names to include (repeatable)
        #[arg(long)]
        part: Vec<String>,

        /// Partition groups to include (repeatable)
        #[arg(long)]
        group: Vec<String>,

        /// Directory containing firmware images
        #[arg(long)]
        firmware_dir: Option<std::path::PathBuf>,

        /// Package root directory
        #[arg(long)]
        package_root: Option<std::path::PathBuf>,

        /// Verify image file existence and size
        #[arg(long)]
        check_images: bool,

        /// Search for images by basename
        #[arg(long)]
        image_search: bool,

        /// Include preloader in dirty-flash mode
        #[arg(long)]
        include_preloader: bool,

        /// Allow incomplete slot pairs
        #[arg(long)]
        allow_incomplete_slots: bool,
    },
}

/// Parse `--mode` string value into [`Mode`].
pub fn parse_mode(s: &str) -> Result<sp::Mode> {
    match s.to_lowercase().as_str() {
        "dry-run" | "dry_run" => Ok(sp::Mode::DryRun),
        "selective" => Ok(sp::Mode::Selective),
        "dirty-flash" | "dirty_flash" => Ok(sp::Mode::DirtyFlash),
        _ => anyhow::bail!("invalid mode '{s}': expected dry-run, selective, or dirty-flash"),
    }
}

/// Parse `--storage` string value into [`StorageSelect`].
pub fn parse_storage(s: &str) -> Result<sp::StorageSelect> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(sp::StorageSelect::Auto),
        "all" => Ok(sp::StorageSelect::All),
        "ufs" => Ok(sp::StorageSelect::Ufs),
        "emmc" => Ok(sp::StorageSelect::Emmc),
        _ => anyhow::bail!("invalid storage '{s}': expected auto, all, ufs, or emmc"),
    }
}
