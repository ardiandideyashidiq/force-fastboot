use std::collections::HashMap;

use tabled::settings::style::Style;
use tabled::{Table, Tabled};

use crate::flash::results::FlashResult;
use crate::scatter_parser::types::{FlashPlan, ScatterFile};

// ── Helpers ──────────────────────────────────────────────────────────

fn colored(mut table: Table) -> String {
    let s = table.with(Style::rounded()).to_string();
    apply_colors(&s)
}

fn apply_colors(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '╭' | '╮' | '╰' | '╯' | '│' | '├' | '┤' | '┬' | '┴' | '─' | '┼' | '╵' => {
                out.push_str(&super::theme::dim(ch.to_string()));
            }
            _ => out.push(ch),
        }
    }
    out
}

// ── Scatter metadata ─────────────────────────────────────────────────

#[derive(Tabled)]
struct MetaRow {
    property: String,
    value: String,
}

/// Display scatter file metadata as a styled table.
#[must_use]
pub fn scatter_metadata(scatter: &ScatterFile) -> String {
    let mut rows: Vec<MetaRow> = Vec::new();

    rows.push(MetaRow {
        property: "Format".into(),
        value: scatter.format.clone(),
    });
    if let Some(platform) = &scatter.platform {
        rows.push(MetaRow {
            property: "Platform".into(),
            value: platform.clone(),
        });
    }
    if let Some(project) = &scatter.project {
        rows.push(MetaRow {
            property: "Project".into(),
            value: project.clone(),
        });
    }
    if let Some(chipset) = scatter.chipset() {
        rows.push(MetaRow {
            property: "Chipset".into(),
            value: chipset,
        });
    }
    rows.push(MetaRow {
        property: "Layouts".into(),
        value: scatter.layouts.len().to_string(),
    });
    let total_parts: usize = scatter.layouts.values().map(Vec::len).sum();
    rows.push(MetaRow {
        property: "Partitions".into(),
        value: total_parts.to_string(),
    });

    if !scatter.warnings.is_empty() {
        rows.push(MetaRow {
            property: super::theme::warn("Warnings"),
            value: scatter.warnings.len().to_string(),
        });
    }
    if !scatter.errors.is_empty() {
        rows.push(MetaRow {
            property: super::theme::error("Errors"),
            value: scatter.errors.len().to_string(),
        });
    }

    colored(Table::new(rows))
}

// ── Scatter warnings / errors ────────────────────────────────────────

#[must_use]
pub fn scatter_warnings(scatter: &ScatterFile) -> Option<String> {
    if scatter.warnings.is_empty() {
        return None;
    }
    let lines: Vec<String> = scatter
        .warnings
        .iter()
        .map(|w| format!("  {} {w}", super::theme::warn("•")))
        .collect();
    Some(lines.join("\n"))
}

#[must_use]
pub fn scatter_errors(scatter: &ScatterFile) -> Option<String> {
    if scatter.errors.is_empty() {
        return None;
    }
    let lines: Vec<String> = scatter
        .errors
        .iter()
        .map(|e| format!("  {} {e}", super::theme::error("•")))
        .collect();
    Some(lines.join("\n"))
}

// ── Flash plan ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct ActionRow {
    #[tabled(rename = "#")]
    number: usize,
    partition: String,
    action: String,
    size: String,
    image: String,
}

#[must_use]
pub fn plan_actions(plan: &FlashPlan) -> String {
    let rows: Vec<ActionRow> = plan
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let img = a.image_resolved_path().unwrap_or("(no image)");
            let short = std::path::Path::new(&img)
                .file_name()
                .map_or_else(|| img.to_string(), |n| n.to_string_lossy().to_string());
            ActionRow {
                number: i + 1,
                partition: a.partition.clone(),
                action: a.action.clone(),
                size: a.size_human.clone(),
                image: short,
            }
        })
        .collect();

    colored(Table::new(rows))
}

#[must_use]
pub fn plan_errors(plan: &FlashPlan) -> Option<String> {
    if plan.errors.is_empty() {
        return None;
    }
    let lines: Vec<String> = plan
        .errors
        .iter()
        .map(|e| format!("  {} {e}", super::theme::error("•")))
        .collect();
    Some(lines.join("\n"))
}

#[must_use]
pub fn plan_warnings(plan: &FlashPlan) -> Option<String> {
    if plan.warnings.is_empty() {
        return None;
    }
    let lines: Vec<String> = plan
        .warnings
        .iter()
        .map(|w| format!("  {} {w}", super::theme::warn("•")))
        .collect();
    Some(lines.join("\n"))
}

#[must_use]
pub fn plan_summary(plan: &FlashPlan) -> String {
    fn check(n: usize, label: &str) -> Option<String> {
        if n > 0 {
            Some(format!("  {label}: {n}"))
        } else {
            None
        }
    }

    let parts: Vec<String> = [
        check(plan.actions.len(), "Flash actions"),
        check(plan.skipped.len(), "Skipped"),
        check(plan.errors.len(), &super::theme::error("Errors")),
        check(plan.warnings.len(), &super::theme::warn("Warnings")),
    ]
    .into_iter()
    .flatten()
    .collect();

    parts.join("\n")
}

// ── Flash skipped partitions ─────────────────────────────────────────

#[must_use]
pub fn plan_skipped(plan: &FlashPlan) -> Option<String> {
    if plan.skipped.is_empty() {
        return None;
    }
    let lines: Vec<String> = plan
        .skipped
        .iter()
        .map(|s| format!("  {} {} — {}", super::theme::dim("•"), s.partition, s.reason))
        .collect();
    Some(lines.join("\n"))
}

// ── Device info ──────────────────────────────────────────────────────

#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn device_info(vars: &HashMap<String, String>) -> String {
    let rows: Vec<MetaRow> = vars
        .iter()
        .map(|(k, v)| MetaRow {
            property: k.clone(),
            value: v.clone(),
        })
        .collect();

    colored(Table::new(rows))
}

// ── Flash results ────────────────────────────────────────────────────

#[must_use]
pub fn flash_result(result: &FlashResult) -> String {
    let succeeded = format!("✓ {} succeeded", result.succeeded);
    let failed = format!("✗ {} failed", result.failed);
    let total = format!("{} total", result.total);
    let sep = "|";

    let header = format!(
        "{}  {}  {}  {}  {}",
        super::theme::ok(&succeeded),
        super::theme::dim(sep),
        super::theme::error(&failed),
        super::theme::dim(sep),
        super::theme::info(&total),
    );

    let mut lines = vec![header];

    for outcome in &result.outcomes {
        if let Some(ref err) = outcome.error {
            lines.push(format!(
                "  {} {} — {}",
                super::theme::error("FAILED"),
                outcome.partition,
                err
            ));
        }
    }

    lines.join("\n")
}

#[must_use]
pub fn format_result(partition: &str, succeeded: usize) -> String {
    let wiped = format!("(wiped: {succeeded})");
    format!(
        "{} {partition}  {}",
        super::theme::ok("✓"),
        super::theme::dim(&wiped),
    )
}
