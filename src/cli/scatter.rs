use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use crate::cli::init_stderr_logging;
use crate::scatter_parser as sp;

/// Parse and print scatter metadata.
pub fn run_parse(path: &Path, full_json: bool) -> Result<()> {
    init_stderr_logging("info");
    let scatter = sp::parse_scatter(path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    if full_json {
        // Print partition-by-partition JSON (similar to --full-json in reference)
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

/// Build and print a flash plan.
pub fn run_plan(
    path: &Path,
    json: bool,
    verbose: bool,
    mode: sp::Mode,
    storage: sp::StorageSelect,
    parts: Vec<String>,
    groups: Vec<String>,
    firmware_dir: Option<PathBuf>,
    package_root: Option<PathBuf>,
    check_images: bool,
    image_search: bool,
    include_preloader: bool,
    allow_incomplete_slots: bool,
) -> Result<()> {
    let level = if verbose { "trace" } else { "info" };
    init_stderr_logging(level);

    info!(?path, "parsing scatter file");
    let scatter = sp::parse_scatter(path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    info!("building flash plan");
    let options = sp::FlashPlanOptions {
        mode,
        storage,
        parts,
        groups,
        firmware_dir,
        package_root,
        check_images,
        image_search,
        include_preloader,
        allow_incomplete_slots,
    };

    let plan = sp::build_flash_plan(&scatter, options);
    info!(actions = plan.actions.len(), skipped = plan.skipped.len(), "plan built");

    if json {
        let output = serde_json::to_string_pretty(&plan)?;
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
