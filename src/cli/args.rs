use clap::{Parser, Subcommand};
use crate::scatter_parser as sp;

#[derive(Parser)]
#[command(name = "pawflash", about = "MTK device flashing toolkit", version)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Force a `MediaTek` device into fastboot mode
    #[command(name = "force-fastboot")]
    ForceFastboot {
        #[arg(short, long)]
        verbose: bool,
    },
    /// Inspect MTK scatter files and build flash plans
    Scatter {
        #[command(subcommand)]
        action: ScatterAction,
    },
    /// Erase and format userdata, cache, metadata with empty filesystems
    #[command(name = "format-data")]
    FormatData {
        #[arg(short, long)]
        verbose: bool,
        /// Comma-separated filesystem options: casefold, projid, compress
        #[arg(long, value_delimiter = ',')]
        fs_options: Vec<String>,
    },
    /// Flash a flash plan to a device over fastboot
    Flash {
        /// Path to the scatter file
        scatter: Option<std::path::PathBuf>,

        /// Dry run: verify device and plan without writing
        #[arg(long)]
        dry_run: bool,

        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,

        /// Flash planning mode
        #[arg(long, default_value = "selective", value_parser = parse_mode)]
        mode: sp::Mode,

        /// Storage layout selection
        #[arg(long, default_value = "auto", value_parser = parse_storage)]
        storage: sp::StorageSelect,

        /// Explicit partition names to include (repeatable)
        #[arg(long)]
        part: Vec<String>,

        /// Partition groups to include (repeatable)
        #[arg(long)]
        group: Vec<String>,

        /// Directory containing firmware images
        #[arg(long)]
        firmware_dir: Option<std::path::PathBuf>,

        /// Verify image file existence and size
        #[arg(long)]
        check_images: bool,

        /// Include preloader in dirty-flash mode
        #[arg(long)]
        include_preloader: bool,
    },
    /// Flash a raw image to a partition (with A/B slot support)
    #[command(name = "flash-raw")]
    FlashRaw {
        /// Partition name (bare, without slot suffix, e.g. boot)
        partition: Option<String>,
        /// Path to the image file
        image: Option<std::path::PathBuf>,
        /// Target slot (a or b); auto-detect from device if not set
        #[arg(long)]
        slot: Option<String>,
        /// Flash to both a and b slots (mutually exclusive with --slot)
        #[arg(long)]
        both: bool,
        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,
    },
    /// Flash empty vbmeta to both slots, disabling dm-verity and AVB verification.
    /// Equivalent to: --disable-verity --disable-verification --slot=all
    #[command(name = "disable-vbmeta")]
    DisableVbmeta {
        /// Enable verbose logging (trace level)
        #[arg(short, long)]
        verbose: bool,
    },
    /// Fastboot device operations
    Device {
        #[arg(short, long)]
        verbose: bool,
        #[command(subcommand)]
        action: DeviceAction,
    },
}

#[derive(Subcommand)]
pub enum ScatterAction {
    /// Print parsed scatter metadata
    Parse {
        scatter: Option<std::path::PathBuf>,
        #[arg(long)]
        full_json: bool,
    },
    /// Build and display a flash plan
    Plan {
        scatter: Option<std::path::PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(short, long)]
        verbose: bool,
        #[arg(long, default_value = "dry-run", value_parser = parse_mode)]
        mode: sp::Mode,
        #[arg(long, default_value = "auto", value_parser = parse_storage)]
        storage: sp::StorageSelect,
        #[arg(long)]
        part: Vec<String>,
        #[arg(long)]
        group: Vec<String>,
        #[arg(long)]
        firmware_dir: Option<std::path::PathBuf>,
        #[arg(long)]
        package_root: Option<std::path::PathBuf>,
        #[arg(long)]
        check_images: bool,
        #[arg(long)]
        image_search: bool,
        #[arg(long)]
        include_preloader: bool,
        #[arg(long)]
        allow_incomplete_slots: bool,
    },
}

#[derive(Subcommand)]
pub enum DeviceAction {
    /// Show device info (all fastboot variables)
    Info,
    /// Reboot the device
    Reboot {
        /// Reboot target: system, bootloader, fastbootd, recovery, bootloader
        #[arg(default_value = "system")]
        target: String,
    },
    /// Lock the bootloader (flashing lock)
    Lock,
    /// Unlock the bootloader (flashing unlock)
    Unlock,
    /// Set the active slot
    #[command(name = "set-active")]
    SetActive {
        /// Slot name: a or b
        slot: String,
    },
    /// Get a fastboot variable
    #[command(name = "get-var")]
    GetVar {
        /// Variable name (e.g., max-download-size, product, version)
        var: String,
    },
}

fn parse_mode(s: &str) -> std::result::Result<sp::Mode, String> {
    match s.to_lowercase().as_str() {
        "dry-run" | "dry_run" => Ok(sp::Mode::DryRun),
        "selective" => Ok(sp::Mode::Selective),
        "dirty-flash" | "dirty_flash" => Ok(sp::Mode::DirtyFlash),
        _ => Err(format!("invalid mode '{s}': expected dry-run, selective, or dirty-flash")),
    }
}

fn parse_storage(s: &str) -> std::result::Result<sp::StorageSelect, String> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(sp::StorageSelect::Auto),
        "all" => Ok(sp::StorageSelect::All),
        "ufs" => Ok(sp::StorageSelect::Ufs),
        "emmc" => Ok(sp::StorageSelect::Emmc),
        _ => Err(format!("invalid storage '{s}': expected auto, all, ufs, or emmc")),
    }
}
