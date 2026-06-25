use std::collections::BTreeSet;

use serde_json::Value;

use crate::scatter_parser::parse::human_size;
use crate::scatter_parser::types::{FlashAction, FlashPlanSummary, ScatterPartition, SkippedPartition};

pub(super) fn flash_action(
    action: &str,
    part: &ScatterPartition,
    image: Option<Value>,
    reason: &str,
    warnings: Vec<String>,
) -> FlashAction {
    FlashAction {
        action: action.to_string(),
        partition: part.name.clone(),
        base_name: part.base_name(),
        slot: part.slot(),
        layout: part.layout.clone(),
        region: part.region.clone(),
        start: part.linear_start,
        start_hex: format!("{:#x}", part.linear_start),
        size: part.size,
        size_hex: format!("{:#x}", part.size),
        size_human: human_size(part.size),
        image,
        image_type: part.image_type.clone(),
        safety_class: part.safety_class(),
        reason: reason.to_string(),
        warnings,
    }
}

pub(super) fn skipped_partition(part: &ScatterPartition, reason: &str) -> SkippedPartition {
    SkippedPartition {
        partition: part.name.clone(),
        layout: part.layout.clone(),
        region: part.region.clone(),
        reason: reason.to_string(),
        safety_class: part.safety_class(),
        file_name: part.file_name.clone(),
    }
}

pub(super) struct PlanSummaryCounts {
    pub(super) skipped: usize,
    pub(super) missing_image: usize,
    pub(super) oversized_image: usize,
    pub(super) action_warnings: usize,
    pub(super) incomplete_slot_bases: usize,
    pub(super) warnings: usize,
    pub(super) errors: usize,
}

pub(super) const fn finalize_plan_summary(
    actions: &[FlashAction],
    counts: &PlanSummaryCounts,
) -> FlashPlanSummary {
    FlashPlanSummary {
        flash_count: actions.len(),
        skipped_count: counts.skipped,
        missing_image_count: counts.missing_image,
        oversized_image_count: counts.oversized_image,
        action_warning_count: counts.action_warnings,
        incomplete_slot_base_count: counts.incomplete_slot_bases,
        warning_count: counts.warnings,
        error_count: counts.errors,
    }
}

pub(super) fn compute_image_counts(actions: &[FlashAction]) -> (usize, usize, usize) {
    let missing_images = actions
        .iter()
        .filter(|a| {
            a.action == "flash"
                && a.image
                    .as_ref()
                    .and_then(|img| img.pointer("/path/exists"))
                    == Some(&Value::Bool(false))
        })
        .count();
    let oversized_images = actions
        .iter()
        .filter(|a| {
            a.action == "flash"
                && a.image
                    .as_ref()
                    .and_then(|img| img.pointer("/status/fits_partition"))
                    == Some(&Value::Bool(false))
        })
        .count();
    let action_warning_count = actions
        .iter()
        .map(|a| a.warnings.len())
        .sum::<usize>();
    (missing_images, oversized_images, action_warning_count)
}

pub(super) fn apply_exclude_filter(
    actions: &mut Vec<FlashAction>,
    skipped: &mut Vec<SkippedPartition>,
    warnings: &mut Vec<String>,
    exclude: &[String],
    available_names: &BTreeSet<String>,
) {
    if exclude.is_empty() {
        return;
    }
    let excluded_names = super::slot::expand_requested_names(exclude, available_names);
    let excluded_set: BTreeSet<_> = excluded_names.iter().collect();
    let (kept, removed): (Vec<_>, Vec<_>) = core::mem::take(actions)
        .into_iter()
        .partition(|a| !excluded_set.contains(&a.partition.to_lowercase()));
    for action in removed {
        skipped.push(SkippedPartition {
            partition: action.partition,
            layout: action.layout,
            region: action.region,
            reason: "excluded by --exclude".into(),
            safety_class: action.safety_class,
            file_name: action.image_type,
        });
    }
    *actions = kept;
    if actions.is_empty() {
        warnings.push("all eligible partitions were excluded by --exclude".into());
    }
}
