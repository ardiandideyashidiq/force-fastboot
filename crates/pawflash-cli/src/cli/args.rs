use clap::{Parser, Subcommand};

use pawflash_core::scatter_parser as sp;

#[derive(Parser)]
#[command(name = "pawflash", about = "MTK device flashing toolkit", version)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Logging verbosity: -v = info, -vv = debug, -vvv = trace
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Expected device serial number; when set, verifies the connected device
    /// matches and rejects non-matching devices.
    #[arg(long, global = true)]
    pub serial: Option<String>,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Force a `MediaTek` device into fastboot mode via preloader serial handshake
    #[command(name = "force-fastboot")]
    ForceFastboot,
    /// Flash operations: scatter-based flash plan, inspect, or raw image flash
    Flash {
        #[command(subcommand)]
        action: Option<FlashAction>,
        /// Partition name (for raw image flash, e.g. boot)
        partition: Option<String>,
        /// Path to the image file (for raw image flash)
        image: Option<std::path::PathBuf>,
        /// Target slot (a or b); auto-detect from device if not set (raw mode only)
        #[arg(long)]
        slot: Option<String>,
        /// Flash to both a and b slots (raw mode only, mutually exclusive with --slot)
        #[arg(long)]
        both: bool,
    },
    /// Flash empty vbmeta to both slots, disabling dm-verity and AVB verification
    #[command(name = "disable-vbmeta")]
    DisableVbmeta,
    /// Erase and format userdata, cache, metadata with empty filesystems
    #[command(name = "format-data")]
    FormatData {
        /// Comma-separated filesystem options: casefold, projid, compress
        #[arg(long, value_delimiter = ',')]
        fs_options: Vec<String>,
        /// Filesystem type for userdata: ext4 or f2fs (default: f2fs)
        #[arg(long, default_value = "f2fs")]
        fs_type: String,
        /// Erase-only: skip filesystem generation (testing)
        #[arg(long)]
        clean_test: bool,
    },
    /// Fastboot device operations
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
}

#[derive(Subcommand)]
pub enum FlashAction {
    /// Inspect a scatter file, build a flash plan, or execute it
    Scatter {
        /// Path to the scatter file
        path: Option<std::path::PathBuf>,
        /// Inspect scatter metadata (omit to build/execute a flash plan)
        #[arg(long)]
        show: bool,
        /// With --show: print all metadata as JSON
        #[arg(long)]
        full_json: bool,
        /// Plan preview only, don't flash (can combine with --json)
        #[arg(long)]
        dry_run: bool,
        /// With --dry-run: output plan as JSON instead of human-readable
        #[arg(long)]
        json: bool,
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
        /// Partition names to exclude from the flash plan (repeatable)
        #[arg(long)]
        exclude: Vec<String>,
        /// Directory containing firmware images
        #[arg(long)]
        firmware_dir: Option<std::path::PathBuf>,
        /// Verify image file existence and size
        #[arg(long)]
        check_images: bool,
        /// Include preloader in dirty-flash mode
        #[arg(long)]
        include_preloader: bool,
        /// Also search adjacent directories for images
        #[arg(long)]
        image_search: bool,
        /// Flash even if some slots are incomplete
        #[arg(long)]
        allow_incomplete_slots: bool,
        /// Flash userdata as well (clean install)
        #[arg(long)]
        clean: bool,
        /// Skip format-data step even when --clean is set
        #[arg(long)]
        no_format: bool,
        /// Erase-only format for clean install (testing, implies --clean)
        #[arg(long)]
        clean_test: bool,
    },
    /// Flash a Generic System Image (GSI) to the device.
    /// Handles vbmeta disable, userdata wipe, and mode transitions
    /// (bootloader ↔ fastbootd) automatically.
    Gsi {
        /// Path to the GSI system image (e.g. system.img)
        image: std::path::PathBuf,
        /// Erase-only format for data partitions (testing)
        #[arg(long)]
        clean_test: bool,
    },
}

#[derive(Debug, Subcommand)]
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
