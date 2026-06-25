//! Flash plan builder — converts a parsed `ScatterFile` into a `FlashPlan`.

pub(crate) mod action;
pub(crate) mod image;
pub(crate) mod layout;
pub(crate) mod mode;
pub(crate) mod slot;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use tracing::debug;

use crate::scatter_parser::types::{
    CleanMode, FlashAction, FlashPlan, FlashPlanOptions, ScatterFile, ScatterPartition,
    SkippedPartition,
};

use self::action::{
    apply_exclude_filter, compute_image_counts, finalize_plan_summary, flash_action,
    PlanSummaryCounts, skipped_partition,
};
use self::image::resolve_images_for_plan;
use self::layout::{selected_layout_names, selected_partitions};
use self::mode::{
    mode_allows_partition, mode_str, record_unknown_groups, select_partition_for_mode,
    storage_str, warn_for_missing_selective_requests,
};
use self::slot::{
    check_incomplete_slots, expand_requested_names, inherited_action_reason,
    inherited_image_source_for_slot_b, synthesize_slot_actions_if_needed,
};

fn build_partition_actions<'a>(
    selected_parts: &'a [ScatterPartition],
    options: &FlashPlanOptions,
    explicit_names: &BTreeSet<String>,
    parts_by_name: &BTreeMap<String, &'a ScatterPartition>,
    scatter_dir: Option<&std::path::Path>,
) -> (Vec<FlashAction>, Vec<SkippedPartition>) {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();

    for part in selected_parts {
        let (selected, selection_reason) =
            select_partition_for_mode(part, options, explicit_names);

        if !selected {
            skipped.push(skipped_partition(part, "not selected"));
            continue;
        }

        if part.slot().is_some() && !part.flashable_by_profile() {
            continue;
        }

        let image_source = inherited_image_source_for_slot_b(part, parts_by_name);
        let (allowed, reason) = mode_allows_partition(
            part,
            image_source,
            options.mode,
            options.allowance.include_preloader,
            options.clean != CleanMode::No,
        );
        if !allowed {
            skipped.push(skipped_partition(part, &reason));
            continue;
        }

        let (image, action_warnings) =
            resolve_images_for_plan(image_source, scatter_dir, options);
        let action_reason = if selection_reason.is_empty() {
            inherited_action_reason(reason, part, image_source)
        } else {
            inherited_action_reason(selection_reason, part, image_source)
        };

        actions.push(flash_action(
            "flash",
            part,
            Some(image),
            &action_reason,
            action_warnings,
        ));
    }

    (actions, skipped)
}

#[must_use]
fn finalize_plan(
    options: &FlashPlanOptions,
    scatter: &ScatterFile,
    actions: &mut Vec<FlashAction>,
    skipped: &mut Vec<SkippedPartition>,
    logs: (&mut Vec<String>, &mut Vec<String>),
    names: (&BTreeSet<String>, &BTreeSet<String>),
    selected_parts: &[ScatterPartition],
) -> FlashPlan {
    let (warnings, errors) = logs;
    let (explicit_names, available_names) = names;
    warn_for_missing_selective_requests(
        options.mode,
        actions,
        explicit_names,
        available_names,
        warnings,
    );

    synthesize_slot_actions_if_needed(selected_parts, actions);

    apply_exclude_filter(actions, skipped, warnings, &options.exclude, available_names);

    let incomplete_slots = check_incomplete_slots(
        selected_parts,
        actions,
        options.allowance.allow_incomplete_slots,
        warnings,
        errors,
    );

    let (missing_images, oversized_images, action_warning_count) =
        compute_image_counts(actions);

    if options.image_verification.check_images && missing_images > 0 {
        errors.push(format!("missing images: {missing_images}"));
    }
    if options.image_verification.check_images && oversized_images > 0 {
        errors.push(format!("oversized images: {oversized_images}"));
    }

    debug!(
        actions = actions.len(),
        skipped = skipped.len(),
        warnings = warnings.len(),
        errors = errors.len(),
        "flash plan summary",
    );

    let summary = finalize_plan_summary(
        actions,
        &PlanSummaryCounts {
            skipped: skipped.len(),
            missing_image: missing_images,
            oversized_image: oversized_images,
            action_warnings: action_warning_count,
            incomplete_slot_bases: incomplete_slots.len(),
            warnings: warnings.len(),
            errors: errors.len(),
        },
    );

    FlashPlan {
        mode: mode_str(options.mode),
        storage_selection: storage_str(options.storage),
        selected_layouts: selected_layout_names(scatter, options.storage),
        platform: scatter.platform.clone(),
        project: scatter.project.clone(),
        firmware_dir: options.firmware_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        package_root: options.package_root.as_ref().map(|p| p.to_string_lossy().into_owned()),
        options: json!({
            "check_images": options.image_verification.check_images,
            "image_search": options.image_verification.image_search,
            "include_preloader": options.allowance.include_preloader,
            "allow_incomplete_slots": options.allowance.allow_incomplete_slots,
            "clean": options.clean != CleanMode::No,
            "parts": options.parts.clone(),
            "groups": options.groups.clone(),
            "exclude": options.exclude.clone(),
        }),
        summary,
        actions: core::mem::take(actions),
        skipped: core::mem::take(skipped),
        incomplete_slots,
        warnings: core::mem::take(warnings),
        errors: core::mem::take(errors),
    }
}

/// Build a safe flash plan for a parsed scatter file.
///
/// # Errors
///
/// Returns [`Error::InvalidValue`] if partition fields cannot be parsed.
#[must_use]
pub fn build_flash_plan(scatter: &ScatterFile, options: &FlashPlanOptions) -> FlashPlan {
    debug!(
        mode = %mode_str(options.mode),
        storage = %storage_str(options.storage),
        parts = options.parts.join(","),
        groups = options.groups.join(","),
        "building flash plan",
    );
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    record_unknown_groups(&options.groups, &mut errors);

    let selected_parts = selected_partitions(scatter, options.storage);
    let parts_by_name = selected_parts
        .iter()
        .map(|part| (part.name.to_lowercase(), part))
        .collect::<BTreeMap<_, _>>();
    let available_names = selected_parts
        .iter()
        .map(|part| part.name.to_lowercase())
        .collect::<BTreeSet<_>>();
    let explicit_names = expand_requested_names(&options.parts, &available_names);

    let scatter_dir = scatter.path.parent();
    let (mut actions, mut skipped) = build_partition_actions(
        &selected_parts,
        options,
        &explicit_names,
        &parts_by_name,
        scatter_dir,
    );

    finalize_plan(
        options,
        scatter,
        &mut actions,
        &mut skipped,
        (&mut warnings, &mut errors),
        (&explicit_names, &available_names),
        &selected_parts,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scatter_parser::types::{Mode, ScatterPartition};

    fn synthetic_part(name: &str, download: bool, has_file: bool, size: i64) -> ScatterPartition {
        ScatterPartition {
            source: "test".to_string(),
            layout: "EMMC".to_string(),
            index: None,
            name: name.to_string(),
            file_name: has_file.then(|| format!("{name}.img")),
            is_download: download,
            image_type: None,
            linear_start: 0,
            physical_start: 0,
            size,
            region: "EMMC_BOOT1".to_string(),
            storage: None,
            boundary_check: true,
            is_reserved: false,
            operation_type: None,
            is_upgradable: None,
            empty_boot_needed: None,
            combo_partsize_check: None,
            raw: json!({}),
            unknown_fields: std::collections::BTreeMap::new(),
        }
    }

    fn userdata_part() -> ScatterPartition {
        ScatterPartition {
            name: "userdata".to_string(),
            size: 0,
            is_download: false,
            file_name: None,
            ..synthetic_part("userdata", false, false, 0)
        }
    }

    fn synthetic_ab_scatter() -> ScatterFile {
        let mut layouts = std::collections::BTreeMap::new();
        layouts.insert(
            "EMMC".to_string(),
            vec![
                synthetic_part("boot_a", true, true, 0x0040_0000),
                synthetic_part("boot_b", false, false, 0x0040_0000),
                synthetic_part("dtbo_a", true, true, 0x0010_0000),
                synthetic_part("dtbo_b", false, false, 0x0010_0000),
                userdata_part(),
            ],
        );
        ScatterFile {
            path: std::path::PathBuf::from("test.xml"),
            format: "xml".to_string(),
            text_hash: "abc".to_string(),
            platform: Some("MT6789".to_string()),
            project: None,
            general: json!({}),
            layouts,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn build_flash_plan_should_select_ufs_layout_by_default() {
        let mut layouts = std::collections::BTreeMap::new();
        layouts.insert("EMMC".to_string(), vec![]);
        layouts.insert(
            "UFS".to_string(),
            vec![synthetic_part("boot", true, true, 0x0040_0000)],
        );
        let scatter = ScatterFile {
            path: std::path::PathBuf::from("test.xml"),
            format: "xml".to_string(),
            text_hash: "abc".to_string(),
            platform: None,
            project: None,
            general: json!({}),
            layouts,
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        let default_options = FlashPlanOptions::default();
        let plan = build_flash_plan(&scatter, &default_options);
        assert_eq!(plan.selected_layouts, vec!["UFS"]);
    }

    #[test]
    fn build_flash_plan_should_error_when_both_slots_are_incomplete() {
        let mut layouts = std::collections::BTreeMap::new();
        layouts.insert(
            "EMMC".to_string(),
            vec![
                synthetic_part("boot_a", true, true, 0x0040_0000),
                synthetic_part("boot_b", true, true, 0x0040_0000),
                synthetic_part("dtbo_a", true, true, 0x0010_0000),
                synthetic_part("dtbo_b", true, true, 0x0010_0000),
                userdata_part(),
            ],
        );
        let scatter = ScatterFile {
            path: std::path::PathBuf::from("test.xml"),
            format: "xml".to_string(),
            text_hash: "abc".to_string(),
            platform: Some("MT6789".to_string()),
            project: None,
            general: json!({}),
            layouts,
            warnings: Vec::new(),
            errors: Vec::new(),
        };
        let options = FlashPlanOptions {
            mode: Mode::Selective,
            parts: vec!["boot_a".to_string()],
            ..FlashPlanOptions::default()
        };
        let plan = build_flash_plan(&scatter, &options);
        assert!(!plan.errors.is_empty(), "expected incomplete slot errors");
        assert!(
            plan.errors.iter().any(|e| e.contains("boot")),
            "error should mention boot: {:?}",
            plan.errors
        );
    }

    #[test]
    fn build_flash_plan_should_synthesize_non_download_b_slots() {
        let scatter = synthetic_ab_scatter();
        let options = FlashPlanOptions {
            mode: Mode::DryRun,
            ..FlashPlanOptions::default()
        };
        let plan = build_flash_plan(&scatter, &options);
        let b_actions: Vec<_> = plan
            .actions
            .iter()
            .filter(|a| a.partition.ends_with("_b"))
            .collect();
        assert!(!b_actions.is_empty(), "expected synthesized slot b actions");
        assert!(
            b_actions.iter().any(|a| a.partition == "boot_b"),
            "expected boot_b: {:?}",
            b_actions.iter().map(|a| &a.partition).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dry_run_should_skip_userdata() {
        let scatter = synthetic_ab_scatter();
        let options = FlashPlanOptions {
            mode: Mode::DryRun,
            ..FlashPlanOptions::default()
        };
        let plan = build_flash_plan(&scatter, &options);
        assert!(
            !plan.actions.iter().any(|a| a.partition == "userdata"),
            "dry run should skip userdata"
        );
    }
}
