use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use pawflash::scatter_parser as sp;

#[derive(Parser)]
#[command(name = "pawflash", about = "MTK device flashing toolkit", version)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Force a `MediaTek` device into fastboot mode
    #[command(name = "force-fastboot")]
    ForceFastboot {
        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,
    },
    /// Inspect MTK scatter files and build flash plans
    Scatter {
        #[command(subcommand)]
        action: ScatterAction,
    },
}

#[derive(Subcommand)]
enum ScatterAction {
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

        /// Flash planning mode
        #[arg(long, default_value = "dry-run")]
        mode: String,

        /// Storage layout selection
        #[arg(long, default_value = "auto")]
        storage: String,

        /// Slot policy
        #[arg(long, default_value = "auto")]
        slot: String,

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

fn parse_mode(s: &str) -> Result<sp::Mode> {
    match s.to_lowercase().as_str() {
        "dry-run" | "dry_run" => Ok(sp::Mode::DryRun),
        "selective" => Ok(sp::Mode::Selective),
        "dirty-flash" | "dirty_flash" => Ok(sp::Mode::DirtyFlash),
        _ => anyhow::bail!("invalid mode '{s}': expected dry-run, selective, or dirty-flash"),
    }
}

fn parse_storage(s: &str) -> Result<sp::StorageSelect> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(sp::StorageSelect::Auto),
        "all" => Ok(sp::StorageSelect::All),
        "ufs" => Ok(sp::StorageSelect::Ufs),
        "emmc" => Ok(sp::StorageSelect::Emmc),
        _ => anyhow::bail!("invalid storage '{s}': expected auto, all, ufs, or emmc"),
    }
}

fn parse_slot(s: &str) -> Result<sp::SlotPolicy> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(sp::SlotPolicy::Auto),
        "a" => Ok(sp::SlotPolicy::A),
        "b" => Ok(sp::SlotPolicy::B),
        "active" => Ok(sp::SlotPolicy::Active),
        "inactive" => Ok(sp::SlotPolicy::Inactive),
        "both" => Ok(sp::SlotPolicy::Both),
        "all-from-scatter" | "all_from_scatter" => Ok(sp::SlotPolicy::AllFromScatter),
        _ => anyhow::bail!("invalid slot policy '{s}'"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand: print help
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
        }
        Some(Commands::ForceFastboot { verbose }) => {
            pawflash::cli::force_fastboot::run(verbose)?;
        }
        Some(Commands::Scatter { action }) => {
            match action {
                ScatterAction::Parse { scatter, full_json } => {
                    pawflash::cli::scatter::run_parse(&scatter, full_json)?;
                }
                ScatterAction::Plan {
                    scatter,
                    json,
                    mode,
                    storage,
                    slot,
                    part,
                    group,
                    firmware_dir,
                    package_root,
                    check_images,
                    image_search,
                    include_preloader,
                    allow_incomplete_slots,
                } => {
                    pawflash::cli::scatter::run_plan(
                        &scatter,
                        json,
                        parse_mode(&mode)?,
                        parse_storage(&storage)?,
                        parse_slot(&slot)?,
                        &part,
                        &group,
                        firmware_dir,
                        package_root,
                        check_images,
                        image_search,
                        include_preloader,
                        allow_incomplete_slots,
                    )?;
                }
            }
        }
    }

    Ok(())
}
