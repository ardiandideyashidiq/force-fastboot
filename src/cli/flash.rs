use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{error, info, warn};

use crate::cli::init_stderr_logging;
use crate::flash::FlashExecutor;
use crate::scatter_parser as sp;

pub async fn run(
    scatter: &PathBuf,
    dry_run: bool,
    verbose: bool,
    mode: sp::Mode,
    storage: sp::StorageSelect,
    parts: Vec<String>,
    groups: Vec<String>,
    firmware_dir: Option<PathBuf>,
    check_images: bool,
    include_preloader: bool,
) -> Result<()> {
    let level = if verbose { "trace" } else { "info" };
    init_stderr_logging(level);

    info!(?scatter, "parsing scatter file");
    let parsed = sp::parse_scatter(scatter)
        .with_context(|| format!("failed to parse {}", scatter.display()))?;

    info!("building flash plan");
    let options = sp::FlashPlanOptions {
        mode,
        storage,
        parts,
        groups,
        firmware_dir,
        package_root: None,
        check_images,
        image_search: false,
        include_preloader,
        allow_incomplete_slots: true,
    };

    let plan = sp::build_flash_plan(&parsed, options);

    if !plan.errors.is_empty() {
        error!("flash plan has errors:");
        for e in &plan.errors {
            error!("  - {e}");
        }
        if !dry_run {
            anyhow::bail!("flash plan errors prevent execution (use --dry-run to see full report)");
        }
    }

    info!(
        actions = plan.actions.len(),
        skipped = plan.skipped.len(),
        "plan built",
    );

    if plan.actions.is_empty() {
        anyhow::bail!("flash plan has no actions to execute");
    }

    info!("connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;

    if dry_run {
        info!("DRY RUN — verifying device match");
        if let Err(e) = executor.verify_device(&plan).await {
            warn!("device verification failed: {e}");
        }
    }

    let result = executor.execute_plan(&plan, dry_run).await;

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
        anyhow::bail!(
            "flash completed with {failed}/{total} failures",
            failed = result.failed,
            total = result.total,
        );
    }

    Ok(())
}
