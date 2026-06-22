use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::scatter_parser as sp;

/// Parse and print scatter metadata.
pub fn run_parse(path: &PathBuf, full_json: bool) -> Result<()> {
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
    path: &PathBuf,
    json: bool,
    mode: sp::Mode,
    storage: sp::StorageSelect,
    slot: sp::SlotPolicy,
    parts: &[String],
    groups: &[String],
    firmware_dir: Option<PathBuf>,
    package_root: Option<PathBuf>,
    check_images: bool,
    image_search: bool,
    include_preloader: bool,
    allow_incomplete_slots: bool,
) -> Result<()> {
    let scatter = sp::parse_scatter(path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    let options = sp::FlashPlanOptions {
        mode,
        storage,
        slot_policy: slot,
        parts: parts.to_vec(),
        groups: groups.to_vec(),
        firmware_dir,
        package_root,
        check_images,
        image_search,
        include_preloader,
        allow_incomplete_slots,
    };

    let plan = sp::build_flash_plan(&scatter, options);

    if json {
        let output = serde_json::to_string_pretty(&plan)?;
        println!("{output}");
    } else {
        println!("Mode:           {}", plan.mode);
        println!("Storage:        {}", plan.storage_selection);
        println!("Layouts:        {}", plan.selected_layouts.join(", "));
        println!("Slot policy:    {} (effective: {})", plan.slot_policy_requested, plan.slot_policy_effective);
        println!("Flash actions:  {}", plan.summary.flash_count);
        println!("Skipped:        {}", plan.summary.skipped_count);

        for action in &plan.actions {
            let img = action.image_resolved_path().unwrap_or("(none)");
            println!(
                "  {} {} -> {} ({})",
                action.action, action.partition, img, action.reason
            );
        }

        if !plan.skipped.is_empty() {
            println!("\nSkipped:");
            for s in &plan.skipped {
                println!("  {}: {}", s.partition, s.reason);
            }
        }

        if !plan.warnings.is_empty() {
            println!("\nWarnings ({}):", plan.warnings.len());
            for w in &plan.warnings {
                println!("  - {w}");
            }
        }
        if !plan.errors.is_empty() {
            eprintln!("\nErrors ({}):", plan.errors.len());
            for e in &plan.errors {
                eprintln!("  - {e}");
            }
        }
    }
    Ok(())
}
