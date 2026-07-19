use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use inquire::{Confirm, Select};
use tracing::info;

use pawflash_core::flash::executor::{BootTarget, FlashExecutor};
use pawflash_core::flash::simulate::SimulatedTransport;
use pawflash_core::output;
use pawflash_core::scatter_parser as sp;

fn show_plan(_parsed: &sp::ScatterFile, plan: &sp::FlashPlan) -> Result<bool> {
    output::status::heading("Interactive Flash Plan");
    output::status::blank();
    output::status::data(output::tables::plan_summary(plan));
    output::status::blank();
    if !plan.actions.is_empty() {
        output::status::data(output::tables::plan_actions(plan));
    }
    if let Some(s) = output::tables::plan_skipped(plan) {
        output::status::blank();
        output::status::data(output::status::dim_colored("Skipped partitions:"));
        output::status::data(s);
    }
    if let Some(w) = output::tables::plan_warnings(plan) {
        output::status::blank();
        output::status::data(output::status::warn_colored("Warnings:"));
        output::status::data(w);
    }

    let has_errors = !plan.errors.is_empty();
    if has_errors {
        output::status::blank();
        output::status::data(output::status::error_colored("Errors:"));
        output::status::stderr(output::tables::plan_errors(plan).unwrap_or_default());
    }

    if has_errors && !Confirm::new("Ignore errors and proceed anyway?").with_default(true).prompt()? {
        output::status::dim("  Aborted.");
        return Ok(false);
    }
    Ok(true)
}

async fn do_reboot<T: pawflash_core::flash::transport::FlashTransport>(executor: &mut FlashExecutor<T>, target: &str) -> Result<()> {
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

/// Format behaviour for the interactive flash flow.
#[derive(Debug, Clone, Copy)]
pub struct FormatConfig {
    pub clean: bool,
    pub no_format: bool,
    pub clean_test: bool,
}

/// Run the interactive flash flow: show plan, confirm, execute with progress,
/// then reboot.
///
/// # Errors
///
/// Returns an error if the scatter file cannot be parsed, the plan cannot
/// be built, the device is not reachable, or any flash operation fails.
pub async fn run(
    scatter_path: &Path,
    exclude: &[String],
    fmt: FormatConfig,
    simulate: bool,
) -> Result<()> {
    let parsed = sp::parse_scatter(scatter_path)
        .with_context(|| format!("failed to parse {}", scatter_path.display()))?;

    let options = sp::FlashPlanOptions {
        mode: sp::Mode::DirtyFlash,
        storage: sp::StorageSelect::Auto,
        image_verification: sp::ImageVerification {
            check_images: true,
            image_search: true,
        },
        exclude: exclude.to_vec(),
        clean: if fmt.clean || fmt.clean_test { sp::CleanMode::Yes } else { sp::CleanMode::No },
        package_root: Some(scatter_path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()),
        ..Default::default()
    };
    let plan = sp::build_flash_plan(&parsed, &options);

    let do_format = !fmt.no_format
        && (fmt.clean || fmt.clean_test)
        && Confirm::new("Format data partitions (userdata, cache, metadata)?").with_default(true).prompt()?;

    if !show_plan(&parsed, &plan)? {
        return Ok(());
    }
    if plan.actions.is_empty() {
        output::status::dim("  Nothing to flash.");
        return Ok(());
    }
    if !Confirm::new("Proceed with flash?").with_default(false).prompt()? {
        output::status::dim("  Aborted.");
        return Ok(());
    }

    info!("connecting to fastboot device");

    if simulate {
        output::status::heading("⚠ SIMULATED MODE — no device will be touched");
        let transport = SimulatedTransport::from_scatter(&parsed);
        let vars = transport.device_vars().clone();
        let mut executor = FlashExecutor::new(transport, vars);
        return execute_interactive_plan(&mut executor, &plan, do_format, fmt.clean_test).await;
    }

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device (60s timeout)...",
        FlashExecutor::wait_for_device(Duration::from_secs(60)),
    )
    .await?;

    execute_interactive_plan(&mut executor, &plan, do_format, fmt.clean_test).await
}

/// Shared execution logic for real and simulated interactive flows.
async fn execute_interactive_plan<T: pawflash_core::flash::transport::FlashTransport>(
    executor: &mut FlashExecutor<T>,
    plan: &sp::FlashPlan,
    do_format: bool,
    clean_test: bool,
) -> Result<()> {
    if do_format {
        output::status::heading("Formatting data partitions");
        let fmt_result = executor.format_data(0, clean_test, None).await?;
        let fmt_failed = pawflash_core::flash::results::print_format_results(&fmt_result);
        if fmt_failed > 0 {
            anyhow::bail!("format-data failed with {fmt_failed} failure(s)");
        }
        output::status::blank();
    }

    let pb = output::spinner::progress_bar(0);
    let result = executor.execute_plan(plan, false, Some(&pb)).await;
    pb.finish_and_clear();

    output::status::blank();
    output::status::data(output::tables::flash_result(&result));

    if result.failed > 0 {
        return Ok(());
    }

    let reboot_target = Select::new("Reboot to:", vec!["none (skip)", "system", "recovery", "bootloader", "fastbootd"]).prompt()?;

    do_reboot(executor, reboot_target).await
}
