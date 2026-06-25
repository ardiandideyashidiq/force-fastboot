pub(crate) mod scatter;
pub(crate) mod raw;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::CommandFactory;
use tracing::warn;

use crate::cli::args::{Cli, FlashAction};
use crate::output;
use crate::scatter_parser as sp;

/// What action to perform with the scatter file.
#[derive(Debug, Clone, Copy)]
enum Action {
    /// Show scatter metadata (replaces `show` + `full_json`).
    Show { full_json: bool },
    /// Dry-run: print plan without executing.
    DryRun,
    /// Execute the flash plan.
    Execute,
}

/// Whether and how to format data partitions.
#[derive(Debug, Clone, Copy)]
enum FormatMode {
    Skip,
    Format,
    Test,
}

/// Grouped config for scatter operations.
struct ScatterConfig<'a> {
    scatter_path: &'a Path,
    action: Action,
    mode: sp::Mode,
    storage: sp::StorageSelect,
    parts: &'a [String],
    groups: &'a [String],
    exclude: &'a [String],
    firmware_dir: Option<&'a Path>,
    image_verification: sp::ImageVerification,
    allowance: sp::Allowance,
    json: bool,
    format_mode: FormatMode,
}

fn print_flash_help() -> Result<()> {
    let mut cmd = Cli::command();
    if let Some(flash) = cmd.find_subcommand_mut("flash") {
        flash.print_help()?;
        output::status::blank();
    }
    Ok(())
}

/// Unified handler for all `pawflash flash` operations.
///
/// # Errors
///
/// Returns an error if the scatter file cannot be parsed, the device
/// is not reachable, or any flash operation fails.
pub async fn run(
    action: Option<FlashAction>,
    partition: Option<String>,
    image: Option<PathBuf>,
    slot: Option<String>,
    both: bool,
) -> Result<()> {
    match action {
        Some(FlashAction::Scatter {
            ref path,
            show,
            full_json,
            dry_run,
            json,
            mode,
            storage,
            ref part,
            ref group,
            ref exclude,
            clean,
            no_format,
            clean_test,
            ref firmware_dir,
            check_images,
            include_preloader,
            image_search,
            allow_incomplete_slots,
        }) => {
            let Some(p) = path else {
                print_flash_help()?;
                return Ok(());
            };
            let scatter_path = p.clone();

            if !show && !dry_run
                && mode == sp::Mode::Selective
                && part.is_empty()
                && group.is_empty()
                && !json
            {
                warn!("no --part/--group specified; interactive mode uses --mode dirty-flash (your --mode {mode:?} is ignored)");
                return crate::cli::interactive::run(&scatter_path, exclude, clean, no_format, clean_test).await;
            }

            let action = if show {
                Action::Show { full_json }
            } else if dry_run {
                Action::DryRun
            } else {
                Action::Execute
            };
            let format_mode = if clean && !no_format || clean_test {
                if clean_test { FormatMode::Test } else { FormatMode::Format }
            } else {
                FormatMode::Skip
            };
            let cfg = ScatterConfig {
                scatter_path: &scatter_path,
                action,
                mode,
                storage,
                parts: part,
                groups: group,
                exclude,
                firmware_dir: firmware_dir.as_deref(),
                image_verification: sp::ImageVerification {
                    check_images,
                    image_search,
                },
                allowance: sp::Allowance {
                    include_preloader,
                    allow_incomplete_slots,
                },
                json,
                format_mode,
            };
            scatter::run_scatter(&cfg).await?;
        }
        Some(FlashAction::Gsi { ref image, clean_test }) => {
            crate::cli::gsi::run(image, clean_test).await?;
        }
        None => {
            let Some(partition) = partition else {
                print_flash_help()?;
                return Ok(());
            };
            let Some(image) = image else {
                print_flash_help()?;
                return Ok(());
            };
            raw::run_raw_image(&partition, &image, slot, both).await?;
        }
    }

    Ok(())
}
