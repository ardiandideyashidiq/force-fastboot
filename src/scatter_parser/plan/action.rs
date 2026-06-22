use serde_json::Value;
use crate::scatter_parser::parse::human_size;
use crate::scatter_parser::types::{
    FlashAction, FlashActionExecutionKind, FlashPlanSummary, ScatterPartition, SkippedPartition,
};

pub(crate) fn flash_action(
    action: &str,
    part: &ScatterPartition,
    image: Option<Value>,
    reason: &str,
    warnings: Vec<String>,
) -> FlashAction {
    FlashAction {
        action: action.to_string(),
        execution_kind: FlashActionExecutionKind::Flash,
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

pub(crate) fn skipped_partition(part: &ScatterPartition, reason: &str) -> SkippedPartition {
    SkippedPartition {
        partition: part.name.clone(),
        layout: part.layout.clone(),
        region: part.region.clone(),
        reason: reason.to_string(),
        safety_class: part.safety_class(),
        file_name: part.file_name.clone(),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PlanSummaryCounts {
    pub(crate) skipped_count: usize,
    pub(crate) missing_image_count: usize,
    pub(crate) oversized_image_count: usize,
    pub(crate) action_warning_count: usize,
    pub(crate) incomplete_slot_base_count: usize,
    pub(crate) warning_count: usize,
    pub(crate) error_count: usize,
}

pub(crate) const fn finalize_plan_summary(
    actions: &[FlashAction],
    counts: PlanSummaryCounts,
) -> FlashPlanSummary {
    FlashPlanSummary {
        flash_count: actions.len(),
        skipped_count: counts.skipped_count,
        missing_image_count: counts.missing_image_count,
        oversized_image_count: counts.oversized_image_count,
        action_warning_count: counts.action_warning_count,
        incomplete_slot_base_count: counts.incomplete_slot_base_count,
        warning_count: counts.warning_count,
        error_count: counts.error_count,
    }
}
