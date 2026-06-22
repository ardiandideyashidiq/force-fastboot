use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use crate::flash::executor::{BootTarget, FlashExecutor};
use crate::flash::results::FormatStatus;
use crate::output;
use crate::output::prompts;
use crate::scatter_parser as sp;

fn show_plan(_parsed: &sp::ScatterFile, plan: &sp::FlashPlan) -> Result<bool> {
    println!("{}", output::theme::heading("Interactive Flash Plan"));
    println!();
    println!("{}", output::tables::plan_summary(plan));
    println!();
    if !plan.actions.is_empty() {
        println!("{}", output::tables::plan_actions(plan));
    }
    if let Some(s) = output::tables::plan_skipped(plan) {
        println!();
        println!("{}", output::theme::dim("Skipped partitions:"));
        println!("{s}");
    }
    if let Some(w) = output::tables::plan_warnings(plan) {
        println!();
        println!("{}", output::theme::warn("Warnings:"));
        println!("{w}");
    }

    let has_errors = !plan.errors.is_empty();
    if has_errors {
        println!();
        println!("{}", output::theme::error("Errors:"));
        println!("{}", output::tables::plan_errors(plan).unwrap_or_default());
    }

    if has_errors && !prompts::confirm_yes("Ignore errors and proceed anyway?")? {
        println!("  {}", output::theme::dim("Aborted."));
        return Ok(false);
    }
    Ok(true)
}

async fn do_format(executor: &mut FlashExecutor) -> Result<()> {
    let format_result = output::spinner::run_with_spinner(
        "Formatting partitions...",
        async { executor.format_data(0).await },
    )
    .await;
    let wiped: usize = format_result
        .outcomes
        .iter()
        .filter(|o| matches!(o.status, FormatStatus::Wiped))
        .count();
    println!("  {}", output::tables::format_result("Format complete", wiped));
    println!();
    Ok(())
}

async fn do_reboot(executor: &mut FlashExecutor, target: &str) -> Result<()> {
    match target {
        "system" => {
            output::spinner::run_with_spinner("Rebooting to system...", async {
                executor.reboot().await
            })
            .await?;
        }
        "recovery" => {
            output::spinner::run_with_spinner("Rebooting to recovery...", async {
                executor.reboot_to(BootTarget::Recovery).await
            })
            .await?;
        }
        "bootloader" => {
            output::spinner::run_with_spinner("Rebooting to bootloader...", async {
                executor.reboot_to(BootTarget::Bootloader).await
            })
            .await?;
        }
        "fastbootd" => {
            output::spinner::run_with_spinner("Rebooting to fastbootd...", async {
                executor.reboot_to(BootTarget::Fastboot).await
            })
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Run the interactive flash flow: show plan, confirm, execute with progress,
/// then optionally format data and reboot.
#[allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]
pub async fn run(scatter_path: &Path, exclude: &[String], clean: bool) -> Result<()> {
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
            clean,
            ..Default::default()
        },
    );

    if !show_plan(&parsed, &plan)? {
        return Ok(());
    }
    if plan.actions.is_empty() {
        println!("  {}", output::theme::dim("Nothing to flash."));
        return Ok(());
    }
    if !prompts::confirm_no("Proceed with flash?")? {
        println!("  {}", output::theme::dim("Aborted."));
        return Ok(());
    }

    info!("connecting to fastboot device");
    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device...",
        FlashExecutor::connect(),
    )
    .await?;

    let pb = output::spinner::progress_bar(0);
    let result = executor.execute_plan(&plan, false, Some(&pb)).await;
    pb.finish_and_clear();

    println!();
    println!("{}", output::tables::flash_result(&result));

    if result.failed > 0 {
        return Ok(());
    }

    if prompts::confirm_no("Format userdata, cache, metadata?")? {
        do_format(&mut executor).await?;
    }

    let reboot_target = prompts::select(
        "Reboot to:",
        vec!["none (skip)", "system", "recovery", "bootloader", "fastbootd"],
    )?;

    do_reboot(&mut executor, reboot_target).await
}
