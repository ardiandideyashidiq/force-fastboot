use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::CommandFactory;
use tracing::{debug, error, info, warn};

use crate::cli::args::{Cli, FlashAction};
use crate::flash::FlashExecutor;
use crate::scatter_parser as sp;

/// Grouped config for scatter operations. Avoids passing 14 individual arguments.
#[expect(clippy::struct_excessive_bools)]
struct ScatterConfig<'a> {
    scatter_path: &'a Path,
    show: bool,
    full_json: bool,
    dry_run: bool,
    json: bool,
    mode: sp::Mode,
    storage: sp::StorageSelect,
    parts: &'a [String],
    groups: &'a [String],
    firmware_dir: Option<&'a Path>,
    check_images: bool,
    include_preloader: bool,
    image_search: bool,
    allow_incomplete_slots: bool,
}

fn print_flash_help() -> Result<()> {
    let mut cmd = Cli::command();
    if let Some(flash) = cmd.find_subcommand_mut("flash") {
        flash.print_help()?;
        println!();
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

            let cfg = ScatterConfig {
                scatter_path: &scatter_path,
                show,
                full_json,
                dry_run,
                json,
                mode,
                storage,
                parts: part,
                groups: group,
                firmware_dir: firmware_dir.as_deref(),
                check_images,
                include_preloader,
                image_search,
                allow_incomplete_slots,
            };
            run_scatter(&cfg).await?;
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
            run_raw_image(&partition, &image, slot, both).await?;
        }
    }

    Ok(())
}

// ── Scatter: show metadata / plan / execute ────────────────────────

async fn run_scatter(cfg: &ScatterConfig<'_>) -> Result<()> {
    debug!(
        scatter_path = %cfg.scatter_path.display(), show = cfg.show, dry_run = cfg.dry_run, ?cfg.mode,
        parts = %cfg.parts.join(","),
        "run_scatter entered",
    );

    // ── Mode 1: Show scatter metadata (was: scatter parse) ──────────
    if cfg.show {
        return show_scatter_metadata(cfg.scatter_path, cfg.full_json);
    }

    // ── Parse scatter and build plan (shared) ──────────────────────
    info!(scatter_path = %cfg.scatter_path.display(), "parsing scatter file");
    let parsed = sp::parse_scatter(cfg.scatter_path)
        .with_context(|| format!("failed to parse {}", cfg.scatter_path.display()))?;
    debug!(
        "scatter parsed: {} partitions across {} layouts",
        parsed.layouts.values().map(Vec::len).sum::<usize>(),
        parsed.layouts.len(),
    );

    let options = sp::FlashPlanOptions {
        mode: cfg.mode,
        storage: cfg.storage,
        parts: cfg.parts.to_vec(),
        groups: cfg.groups.to_vec(),
        firmware_dir: cfg.firmware_dir.map(Path::to_path_buf),
        package_root: None,
        check_images: cfg.check_images,
        image_search: cfg.image_search,
        include_preloader: cfg.include_preloader,
        allow_incomplete_slots: cfg.allow_incomplete_slots,
    };

    info!("building flash plan");
    let plan = sp::build_flash_plan(&parsed, options);
    debug!(actions = plan.actions.len(), skipped = plan.skipped.len(), "flash plan built");

    if !plan.errors.is_empty() {
        error!("flash plan has errors:");
        for e in &plan.errors {
            error!("  - {e}");
        }
        if !cfg.dry_run {
            bail!("flash plan errors prevent execution (use --dry-run to see full report)");
        }
    }

    if plan.actions.is_empty() && !cfg.dry_run {
        bail!("flash plan has no actions to execute");
    }

    // ── Mode 2: Dry run — print plan (was: scatter plan) ──────────
    if cfg.dry_run {
        return print_plan(&plan, cfg.json);
    }

    // ── Mode 3: Execute (was: flash) ────────────────────────────────
    info!(
        actions = plan.actions.len(),
        skipped = plan.skipped.len(),
        "plan built",
    );

    info!("connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;
    debug!("connected, executing flash plan");

    let result = executor.execute_plan(&plan, false).await;

    info!(
        total = result.total,
        succeeded = result.succeeded,
        failed = result.failed,
        "flash execution summary",
    );

    for outcome in &result.outcomes {
        if let Some(ref err) = outcome.error {
            error!(partition = outcome.partition, error = %err, "flash failed");
        }
    }

    if result.failed > 0 {
        bail!(
            "flash completed with {failed}/{total} failures",
            failed = result.failed,
            total = result.total,
        );
    }

    Ok(())
}

// ── Scatter: show metadata ─────────────────────────────────────────

fn show_scatter_metadata(path: &Path, full_json: bool) -> Result<()> {
    let scatter = sp::parse_scatter(path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    if full_json {
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "path": scatter.path,
            "format": scatter.format,
            "sha256_text": scatter.text_hash,
            "platform": scatter.platform,
            "project": scatter.project,
            "chipset": scatter.chipset(),
            "layout_names": scatter.layouts.keys().collect::<Vec<_>>(),
            "partition_count": scatter.layouts.values().map(Vec::len).sum::<usize>(),
            "warnings": scatter.warnings,
            "errors": scatter.errors,
        }))?;
        println!("{output}");
    } else {
        println!("Scatter: {}", scatter.path.display());
        println!("Format:  {}", scatter.format);
        println!("Hash:    {}", scatter.text_hash);
        if let Some(platform) = &scatter.platform {
            println!("Platform: {platform}");
        }
        if let Some(project) = &scatter.project {
            println!("Project:  {project}");
        }
        if let Some(chipset) = scatter.chipset() {
            println!("Chipset:  {chipset}");
        }
        println!("Layouts:  {}", scatter.layouts.len());
        for (layout, parts) in &scatter.layouts {
            println!("  {layout}: {} partitions", parts.len());
        }
        if !scatter.warnings.is_empty() {
            println!("\nWarnings ({}):", scatter.warnings.len());
            for w in &scatter.warnings {
                println!("  - {w}");
            }
        }
        if !scatter.errors.is_empty() {
            eprintln!("\nErrors ({}):", scatter.errors.len());
            for e in &scatter.errors {
                eprintln!("  - {e}");
            }
        }
    }

    Ok(())
}

// ── Scatter: print plan ────────────────────────────────────────────

fn print_plan(plan: &sp::FlashPlan, json: bool) -> Result<()> {
    if json {
        let output = serde_json::to_string_pretty(plan)?;
        println!("{output}");
    } else {
        info!(
            mode = plan.mode,
            layouts = %plan.selected_layouts.join(","),
            platform = plan.platform.as_deref().unwrap_or("(unknown)"),
            project = plan.project.as_deref().unwrap_or("(unknown)"),
            flash_actions = plan.summary.flash_count,
            skipped = plan.summary.skipped_count,
            "plan summary",
        );

        for action in &plan.actions {
            let img = action.image_resolved_path().unwrap_or("(none)");
            info!(
                action = action.action,
                partition = action.partition,
                image = img,
                reason = action.reason,
                "flash action",
            );
        }

        for s in &plan.skipped {
            info!(
                partition = s.partition,
                reason = s.reason,
                "skipped partition",
            );
        }

        for w in &plan.warnings {
            warn!("{w}");
        }
        for e in &plan.errors {
            error!("{e}");
        }
    }

    Ok(())
}

// ── Raw image flash (was: flash-raw) ────────────────────────────────

async fn run_raw_image(
    partition: &str,
    image: &Path,
    slot: Option<String>,
    both: bool,
) -> Result<()> {
    if both && slot.is_some() {
        bail!("--both and --slot are mutually exclusive");
    }
    if let Some(ref s) = slot {
        if s != "a" && s != "b" {
            bail!("--slot must be 'a' or 'b', got '{s}'");
        }
    }

    if !image.exists() {
        bail!("image not found: {}", image.display());
    }
    let image = image.canonicalize().context("failed to resolve image path")?;

    debug!(%partition, image = %image.display(), ?slot, both, "raw image flash requested");
    info!(
        partition,
        image = %image.display(),
        ?slot,
        both,
        "connecting to fastboot device",
    );

    let mut executor = FlashExecutor::connect().await?;

    let targets: Vec<String> = if both {
        vec![format!("{partition}_a"), format!("{partition}_b")]
    } else if let Some(s) = slot {
        vec![format!("{partition}_{s}")]
    } else {
        let current = executor.device_vars().get("current-slot").map(String::as_str);
        if let Some(slot) = current {
            vec![format!("{partition}_{slot}")]
        } else {
            warn!("device has no current-slot variable; flashing to bare partition name");
            vec![partition.to_string()]
        }
    };

    info!(?targets, partition, "flashing");

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for target in &targets {
        match executor.flash_raw_image(target, &image).await {
            Ok(()) => {
                info!(partition = %target, "flash successful");
                succeeded += 1;
            }
            Err(e) => {
                error!(partition = %target, error = %e, "flash failed");
                failed += 1;
            }
        }
    }

    info!(succeeded, failed, "flash complete");

    if failed > 0 && succeeded == 0 {
        bail!("flash-raw failed for all targets");
    }

    Ok(())
}
