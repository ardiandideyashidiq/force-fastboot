use std::path::Path;

use anyhow::{bail, Context, Result};
use tracing::{debug, info};

use pawflash_core::flash::FlashExecutor;
use pawflash_core::output;
use pawflash_core::scatter_parser as sp;

use super::{Action, FormatMode, ScatterConfig};

pub(super) async fn run_scatter(cfg: &ScatterConfig<'_>) -> Result<()> {
    debug!(
        scatter_path = %cfg.scatter_path.display(), ?cfg.action, ?cfg.mode,
        parts = %cfg.parts.join(","),
        "run_scatter entered",
    );

    // ── Mode 1: Show scatter metadata ────────────────────────────
    if let Action::Show { full_json } = cfg.action {
        return show_scatter_metadata(cfg.scatter_path, full_json);
    }

    // ── Parse scatter and build plan (shared) ────────────────────
    info!(scatter_path = %cfg.scatter_path.display(), "parsing scatter file");
    let parsed = sp::parse_scatter(cfg.scatter_path)
        .with_context(|| format!("failed to parse {}", cfg.scatter_path.display()))?;
    debug!(
        "scatter parsed: {} partitions across {} layouts",
        parsed.layouts.values().map(Vec::len).sum::<usize>(),
        parsed.layouts.len(),
    );

    let is_dry_run = matches!(cfg.action, Action::DryRun);
    let formatted_on_execute = match cfg.format_mode {
        FormatMode::Skip => None,
        FormatMode::Format => Some(false),
        FormatMode::Test => Some(true),
    };
    let is_clean = formatted_on_execute.is_some();

    let options = sp::FlashPlanOptions {
        mode: cfg.mode,
        storage: cfg.storage,
        parts: cfg.parts.to_vec(),
        groups: cfg.groups.to_vec(),
        exclude: cfg.exclude.to_vec(),
        firmware_dir: cfg.firmware_dir.map(Path::to_path_buf),
        package_root: Some(cfg.scatter_path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()),
        image_verification: cfg.image_verification,
        allowance: cfg.allowance,
        clean: if is_clean { sp::CleanMode::Yes } else { sp::CleanMode::No },
    };

    info!("building flash plan");
    let plan = sp::build_flash_plan(&parsed, &options);
    debug!(actions = plan.actions.len(), skipped = plan.skipped.len(), "flash plan built");

    // Show errors early (before bail! so user sees them)
    if !plan.errors.is_empty() {
        output::status::stderr(output::tables::plan_errors(&plan).unwrap_or_default());
        if !is_dry_run {
            bail!("flash plan errors prevent execution (use --dry-run to see full report)");
        }
    }

    if plan.actions.is_empty() && !is_dry_run {
        bail!("flash plan has no actions to execute");
    }

    // ── Mode 2: Dry run ─────────────────────────────────────────
    if is_dry_run {
        return print_plan(&plan, cfg.json);
    }

    // ── Mode 3: Execute ─────────────────────────────────────────
    info!(
        actions = plan.actions.len(),
        skipped = plan.skipped.len(),
        "plan built",
    );

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device...",
        FlashExecutor::connect(),
    )
    .await?;

    // ── Optional: format data partitions (--clean/--clean-test) ────
    let clean_test = formatted_on_execute.unwrap_or(false);
    if is_clean {
        output::status::heading("Formatting data partitions");
        let fmt_result = executor.format_data(0, clean_test, None).await?;
        let fmt_failed = pawflash_core::flash::results::print_format_results(&fmt_result);
        if fmt_failed > 0 {
            bail!("format-data failed with {fmt_failed} failure(s)");
        }
        output::status::blank();
    }

    debug!("connected, executing flash plan");

    let result = executor.execute_plan(&plan, false, None).await;

    info!(
        total = result.total,
        succeeded = result.succeeded,
        failed = result.failed,
        "flash execution summary",
    );

    if cfg.json {
        let json_output = serde_json::to_string_pretty(&result)?;
        output::status::data(&json_output);
    } else {
        output::status::stderr(output::tables::flash_result(&result));
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
        output::status::data(&output);
    } else {
        output::status::data(output::tables::scatter_metadata(&scatter));
        if let Some(w) = output::tables::scatter_warnings(&scatter) {
            output::status::blank();
            output::status::data(output::status::warn_colored("Warnings:"));
            output::status::data(w);
        }
        if let Some(e) = output::tables::scatter_errors(&scatter) {
            output::status::blank();
            output::status::data(output::status::error_colored("Errors:"));
            output::status::data(e);
        }
    }

    Ok(())
}

fn print_plan(plan: &sp::FlashPlan, json: bool) -> Result<()> {
    if json {
        let output = serde_json::to_string_pretty(plan)?;
        output::status::data(&output);
    } else {
        output::status::heading("Flash Plan");
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
        if let Some(e) = output::tables::plan_errors(plan) {
            output::status::blank();
            output::status::data(output::status::error_colored("Errors:"));
            output::status::data(e);
        }
    }

    Ok(())
}
