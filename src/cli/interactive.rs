use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use tracing::info;

use crate::flash::executor::{BootTarget, FlashExecutor};
use crate::flash::results::FormatStatus;
use crate::scatter_parser as sp;

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt} ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn confirm(prompt: &str) -> Result<bool> {
    let line = read_line(&format!("{prompt} [Y/n]"))?;
    Ok(line.is_empty() || line.eq_ignore_ascii_case("y") || line.eq_ignore_ascii_case("yes"))
}

fn confirm_n(prompt: &str) -> Result<bool> {
    let line = read_line(&format!("{prompt} [y/N]"))?;
    Ok(!line.is_empty() && (line.eq_ignore_ascii_case("y") || line.eq_ignore_ascii_case("yes")))
}

fn display_image_name(action: &sp::FlashAction) -> String {
    let path = action.image_resolved_path();
    path.and_then(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| path.map_or_else(|| "(no image)".to_string(), ToString::to_string))
}

fn show_plan(parsed: &sp::ScatterFile, plan: &sp::FlashPlan) -> bool {
    eprintln!("  ─────────────────────────────────────────────");
    eprintln!("  pawflash interactive flash");
    eprintln!("  ─────────────────────────────────────────────");
    eprintln!();
    eprintln!("  Scatter:  {}", parsed.path.display());
    if let Some(platform) = &parsed.platform {
        eprintln!("  Platform: {platform}");
    }
    if let Some(project) = &parsed.project {
        eprintln!("  Project:  {project}");
    }
    for (layout, parts) in &parsed.layouts {
        eprintln!("  Layout:   {layout} ({} partitions)", parts.len());
    }
    eprintln!();
    eprintln!(
        "  Plan:     dirty-flash — {} flash actions, {} skipped",
        plan.summary.flash_count,
        plan.summary.skipped_count,
    );
    eprintln!();

    for (i, action) in plan.actions.iter().enumerate() {
        let img = display_image_name(action);
        eprintln!(
            "  {:>3}. {:<18} {:>8}  {}",
            i + 1,
            action.partition,
            action.size_human,
            img,
        );
    }

    eprintln!();

    plan.errors.is_empty() || confirm("Ignore errors and proceed anyway?").unwrap_or(false)
}

async fn handle_flash_result(
    result: &crate::flash::results::FlashResult,
    executor: &mut FlashExecutor,
) -> Result<()> {
    eprintln!();
    eprintln!(
        "  Flash complete: {}/{} succeeded, {} failed",
        result.succeeded, result.total, result.failed,
    );
    eprintln!();

    if result.failed > 0 {
        for outcome in &result.outcomes {
            if let Some(ref err) = outcome.error {
                eprintln!("  FAILED: {} — {err}", outcome.partition);
            }
        }
        eprintln!();
    }

    if result.succeeded > 0 && confirm_n("Format userdata, cache, metadata?")? {
        eprintln!("  Formatting...");
        let format_result = executor.format_data(0).await;
        let wiped = format_result
            .outcomes
            .iter()
            .filter(|o| matches!(o.status, FormatStatus::Wiped))
            .count();
        eprintln!("  Format complete: {wiped} partitions wiped.");
        eprintln!();
    }

    eprintln!("  Reboot options: s=system  r=recovery  b=bootloader  f=fastbootd  (enter=none)");
    let line = read_line("  Reboot?")?;
    match line.to_lowercase().as_str() {
        "s" | "system" => {
            executor.reboot().await?;
            eprintln!("  Rebooting to system.");
        }
        "r" | "recovery" => {
            executor.reboot_to(BootTarget::Recovery).await?;
            eprintln!("  Rebooting to recovery.");
        }
        "b" | "bootloader" => {
            executor.reboot_to(BootTarget::Bootloader).await?;
            eprintln!("  Rebooting to bootloader.");
        }
        "f" | "fastbootd" | "fastboot" => {
            executor.reboot_to(BootTarget::Fastboot).await?;
            eprintln!("  Rebooting to fastbootd.");
        }
        _ => {
            eprintln!("  Skipping reboot.");
        }
    }

    Ok(())
}

/// Run the interactive flash flow: show plan, confirm, execute with progress,
/// then optionally format data and reboot.
#[allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]
pub async fn run(scatter_path: &Path, exclude: &[String]) -> Result<()> {
    let parsed = sp::parse_scatter(scatter_path)
        .with_context(|| format!("failed to parse {}", scatter_path.display()))?;

    let plan = sp::build_flash_plan(
        &parsed,
        sp::FlashPlanOptions {
            mode: sp::Mode::DirtyFlash,
            storage: sp::StorageSelect::Auto,
            check_images: true,
            image_search: true,
            exclude: exclude.to_vec(),
            ..Default::default()
        },
    );

    if !show_plan(&parsed, &plan) {
        eprintln!("  Aborted.");
        return Ok(());
    }

    if plan.actions.is_empty() {
        eprintln!("  Nothing to flash.");
        return Ok(());
    }

    if !confirm("Proceed with flash?")? {
        eprintln!("  Aborted.");
        return Ok(());
    }

    eprintln!();
    info!("connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;
    eprintln!("  Connected.");
    eprintln!();

    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:>16}: [{bar:40.green/red}] {bytes}/{total_bytes}  {bytes_per_sec}  ETA {eta}  [{elapsed_precise}]",
        )
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ "),
    );

    let result = executor.execute_plan(&plan, false, Some(&pb)).await;
    pb.finish_and_clear();

    handle_flash_result(&result, &mut executor).await
}