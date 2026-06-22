//! Flash plan builder — converts a parsed `ScatterFile` into a `FlashPlan`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::scatter_parser::parse::{human_size, image_magic, resolve_image_path};
use crate::scatter_parser::safety::{
    BOOTLOADER_CANONICAL, BOOT_CHAIN_CANONICAL, MODEM_CANONICAL, MCU_FW_CANONICAL,
    ANDROID_CANONICAL, REGIONAL_CANONICAL,
};
use crate::scatter_parser::types::{
    FlashAction, FlashActionExecutionKind, FlashPlan, FlashPlanOptions, FlashPlanSummary,
    Mode, ScatterFile, ScatterPartition, SkippedPartition, StorageSelect,
    split_base_slot,
};

/// Build a safe flash plan for a parsed scatter file.
///
/// # Errors
///
/// Returns [`Error::InvalidValue`] if partition fields cannot be parsed.
// Takes `options` by value: fields are moved into the plan without cloning.
#[expect(clippy::needless_pass_by_value)]
pub fn build_flash_plan(scatter: &ScatterFile, options: FlashPlanOptions) -> FlashPlan {
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
    let explicit_names =
        expand_requested_names(&options.parts, &available_names);

    let scatter_dir = scatter.path.parent();
    let mut actions = Vec::new();
    let mut skipped = Vec::new();

    for part in &selected_parts {
        let (selected, selection_reason) =
            select_partition_for_mode(part, &options, &explicit_names);

        if !selected {
            skipped.push(skipped_partition(part, "not selected"));
            continue;
        }

        if part.slot().is_some() && !part.flashable_by_profile() {
            // Non-download slot partition; will be synthesized from sibling later
            continue;
        }

        let image_source =
            inherited_image_source_for_slot_b(part, &parts_by_name);
        let (allowed, reason) =
            mode_allows_partition(part, image_source, options.mode, options.include_preloader);
        if !allowed {
            skipped.push(skipped_partition(part, &reason));
            continue;
        }

        let (image, action_warnings) =
            resolve_images_for_plan(image_source, scatter_dir, &options);
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

    warn_for_missing_selective_requests(
        options.mode,
        &actions,
        &explicit_names,
        &available_names,
        &mut warnings,
    );

    synthesize_slot_actions_if_needed(&selected_parts, &mut actions);

    let incomplete_slots = check_incomplete_slots(
        &selected_parts,
        &actions,
        options.allow_incomplete_slots,
        &mut warnings,
        &mut errors,
    );

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

    if options.check_images && missing_images > 0 {
        errors.push(format!("missing images: {missing_images}"));
    }
    if options.check_images && oversized_images > 0 {
        errors.push(format!("oversized images: {oversized_images}"));
    }

    let summary = finalize_plan_summary(&actions, PlanSummaryCounts {
        skipped_count: skipped.len(),
        missing_image_count: missing_images,
        oversized_image_count: oversized_images,
        action_warning_count,
        incomplete_slot_base_count: incomplete_slots.len(),
        warning_count: warnings.len(),
        error_count: errors.len(),
    });

    FlashPlan {
        mode: mode_str(options.mode),
        storage_selection: storage_str(options.storage),
        selected_layouts: selected_layout_names(scatter, options.storage),
        platform: scatter.platform.clone(),
        project: scatter.project.clone(),
        firmware_dir: options.firmware_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        package_root: options.package_root.as_ref().map(|p| p.to_string_lossy().into_owned()),
        options: json!({
            "check_images": options.check_images,
            "image_search": options.image_search,
            "include_preloader": options.include_preloader,
            "allow_incomplete_slots": options.allow_incomplete_slots,
            "parts": options.parts,
            "groups": options.groups,
        }),
        summary,
        actions,
        skipped,
        incomplete_slots,
        warnings,
        errors,
    }
}

// --- Layout selection ---

fn selected_partitions(
    scatter: &ScatterFile,
    storage: StorageSelect,
) -> Vec<ScatterPartition> {
    selected_layouts(scatter, storage)
        .into_values()
        .flatten()
        .collect()
}

fn selected_layouts(
    scatter: &ScatterFile,
    storage: StorageSelect,
) -> BTreeMap<String, Vec<ScatterPartition>> {
    if storage == StorageSelect::All {
        return scatter.layouts.clone();
    }

    let upper_to_key = scatter
        .layouts
        .keys()
        .map(|key| (key.to_uppercase(), key.clone()))
        .collect::<BTreeMap<_, _>>();

    match storage {
        StorageSelect::Ufs => upper_to_key
            .get("UFS")
            .map(|key| BTreeMap::from([(key.clone(), scatter.layouts[key].clone())]))
            .unwrap_or_default(),
        StorageSelect::Emmc => upper_to_key
            .get("EMMC")
            .map(|key| BTreeMap::from([(key.clone(), scatter.layouts[key].clone())]))
            .unwrap_or_default(),
        StorageSelect::Auto => {
            for wanted in ["UFS", "EMMC"] {
                if let Some(key) = upper_to_key.get(wanted) {
                    return BTreeMap::from([(key.clone(), scatter.layouts[key].clone())]);
                }
            }
            scatter
                .layouts
                .iter()
                .next()
                .map(|(key, parts)| BTreeMap::from([(key.clone(), parts.clone())]))
                .unwrap_or_default()
        }
        StorageSelect::All => unreachable!("handled by early return"),
    }
}

fn selected_layout_names(
    scatter: &ScatterFile,
    storage: StorageSelect,
) -> Vec<String> {
    selected_layouts(scatter, storage).keys().cloned().collect()
}

// --- Mode helpers ---

fn mode_str(mode: Mode) -> String {
    match mode {
        Mode::DryRun => "dry-run",
        Mode::Selective => "selective",
        Mode::DirtyFlash => "dirty-flash",
    }
    .to_string()
}

fn storage_str(storage: StorageSelect) -> String {
    match storage {
        StorageSelect::Auto => "auto",
        StorageSelect::All => "all",
        StorageSelect::Ufs => "ufs",
        StorageSelect::Emmc => "emmc",
    }
    .to_string()
}

// --- Partition selection ---

fn select_partition_for_mode(
    part: &ScatterPartition,
    options: &FlashPlanOptions,
    explicit_names: &BTreeSet<String>,
) -> (bool, String) {
    match options.mode {
        Mode::DryRun | Mode::DirtyFlash => {
            (true, format!("mode {}", mode_str(options.mode)))
        }
        Mode::Selective => {
            let by_part = explicit_names.contains(&part.name.to_lowercase())
                || explicit_names.contains(&part.base_name().to_lowercase())
                || explicit_names.contains(&part.canonical());
            let by_group = part_matches_group(part, &options.groups);
            let reason = match (by_part, by_group) {
                (true, true) => "selected by part and group",
                (true, false) => "selected by part",
                (false, true) => "selected by group",
                (false, false) => "",
            };
            (by_part || by_group, reason.to_string())
        }
    }
}

fn mode_allows_partition(
    part: &ScatterPartition,
    image_source: &ScatterPartition,
    mode: Mode,
    include_preloader: bool,
) -> (bool, String) {
    let canonical = part.canonical();
    let safety = part.safety_class();
    let flashable = image_source.flashable_by_profile() && part.size > 0;

    if matches!(safety.as_str(), "identity_or_calibration" | "dangerous") {
        return (false, format!("blocked safety class: {safety}"));
    }
    if canonical == "preloader" && !include_preloader {
        return (false, "preloader requires --include-preloader".to_string());
    }

    match mode {
        Mode::DryRun => {
            if flashable {
                (true, "scatter profile selected".to_string())
            } else {
                (false, "not selected by scatter profile or no image".to_string())
            }
        }
        Mode::Selective => {
            if flashable {
                (true, "selected by user".to_string())
            } else {
                (false, "selected but not flashable by scatter profile".to_string())
            }
        }
        Mode::DirtyFlash => {
            if !flashable {
                return (false, "not selected by scatter profile or no image".to_string());
            }
            if BOOTLOADER_CANONICAL.contains(&canonical.as_str())
                || BOOT_CHAIN_CANONICAL.contains(&canonical.as_str())
                || MODEM_CANONICAL.contains(&canonical.as_str())
                || MCU_FW_CANONICAL.contains(&canonical.as_str())
                || ANDROID_CANONICAL.contains(&canonical.as_str())
                || REGIONAL_CANONICAL.contains(&canonical.as_str())
            {
                (true, format!("allowed by {}", mode_str(mode)))
            } else {
                (false, format!("not included in {} policy", mode_str(mode)))
            }
        }
    }
}

// --- Image resolution ---

fn resolve_images_for_plan(
    part: &ScatterPartition,
    scatter_dir: Option<&Path>,
    options: &FlashPlanOptions,
) -> (Value, Vec<String>) {
    let resolved = resolve_image_path(
        part.file_name.as_deref(),
        scatter_dir,
        options.firmware_dir.as_deref(),
        options.package_root.as_deref(),
        options.image_search,
    );
    let (status, mut warnings) = checked_image_status(
        resolved.resolved_path.as_deref(),
        resolved.exists,
        options.check_images,
        part.size,
    );
    if let Some(warning) = &resolved.warning {
        warnings.insert(0, warning.clone());
    }
    (
        json!({
            "file_name": part.file_name,
            "path": resolved,
            "status": status,
        }),
        warnings,
    )
}

fn checked_image_status(
    resolved_path: Option<&str>,
    exists: Option<bool>,
    checked: bool,
    target_size: i64,
) -> (Value, Vec<String>) {
    let mut warnings = Vec::new();
    let mut status = json!({
        "checked": checked,
        "exists": exists,
        "size_bytes": null,
        "size_human": null,
        "fits_partition": null,
        "magic": null,
    });
    if !checked {
        return (status, warnings);
    }

    if let Some(path) = resolved_path.filter(|_| exists == Some(true)) {
        match std::fs::metadata(path) {
            Ok(meta) => {
                if let Ok(size) = i64::try_from(meta.len()) {
                    status["size_bytes"] = json!(size);
                    status["size_human"] = json!(human_size(size));
                    status["fits_partition"] = json!(size <= target_size);
                    status["magic"] = json!(image_magic(std::path::Path::new(path)));
                    if size > target_size {
                        warnings.push(format!(
                            "image is larger than partition: {size} > {target_size}"
                        ));
                    }
                }
            }
            Err(e) => warnings.push(format!("failed to stat image: {e}")),
        }
    } else {
        warnings.push("image missing".to_string());
    }
    (status, warnings)
}

// --- Slot helpers ---

fn inherited_image_source_for_slot_b<'a>(
    part: &'a ScatterPartition,
    parts_by_name: &BTreeMap<String, &'a ScatterPartition>,
) -> &'a ScatterPartition {
    if part.slot().as_deref() != Some("b")
        || part.flashable_by_profile()
    {
        return part;
    }

    let source_name = format!("{}_a", part.base_name());
    match parts_by_name.get(&source_name) {
        Some(source) if source.flashable_by_profile() => source,
        _ => part,
    }
}

fn inherited_action_reason(
    base_reason: String,
    part: &ScatterPartition,
    image_source: &ScatterPartition,
) -> String {
    if part.name.eq_ignore_ascii_case(&image_source.name) {
        return base_reason;
    }
    let Some(source_slot) = image_source.slot() else {
        return base_reason;
    };
    format!("{base_reason}; inherited from slot {source_slot} image")
}

fn expand_requested_names(
    requested: &[String],
    available: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in requested {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let lname = name.to_lowercase();
        if available.contains(&lname) || split_base_slot(&lname).1.is_some() {
            out.insert(lname);
            continue;
        }
        let mut added = false;
        for slot in ["a", "b"] {
            let candidate = format!("{lname}_{slot}");
            if available.contains(&candidate) {
                out.insert(candidate);
                added = true;
            }
        }
        if !added {
            out.insert(lname);
        }
    }
    out
}

// --- Group helpers ---

fn part_matches_group(part: &ScatterPartition, groups: &[String]) -> bool {
    let canonical = part.canonical();
    groups.iter().any(|group| group_members(group).contains(&canonical.as_str()))
}

fn group_names() -> BTreeSet<&'static str> {
    [
        "boot", "bootloader", "avb", "modem", "mcu", "firmware",
        "android", "regional", "full-safe",
    ]
    .into_iter()
    .collect()
}

fn group_members(group: &str) -> BTreeSet<&'static str> {
    match group.trim().to_lowercase().as_str() {
        "boot" => BOOT_CHAIN_CANONICAL.iter().copied().collect(),
        "bootloader" => BOOTLOADER_CANONICAL.iter().copied().collect(),
        "avb" => ["vbmeta", "vbmeta_system", "vbmeta_vendor"].into_iter().collect(),
        "modem" => MODEM_CANONICAL.iter().copied().collect(),
        "mcu" => MCU_FW_CANONICAL.iter().copied().collect(),
        "firmware" => BOOTLOADER_CANONICAL
            .iter()
            .chain(BOOT_CHAIN_CANONICAL)
            .chain(MODEM_CANONICAL)
            .chain(MCU_FW_CANONICAL)
            .copied()
            .collect(),
        "android" => ANDROID_CANONICAL.iter().copied().collect(),
        "regional" => REGIONAL_CANONICAL.iter().copied().collect(),
        "full-safe" => BOOTLOADER_CANONICAL
            .iter()
            .chain(BOOT_CHAIN_CANONICAL)
            .chain(MODEM_CANONICAL)
            .chain(MCU_FW_CANONICAL)
            .chain(ANDROID_CANONICAL)
            .chain(REGIONAL_CANONICAL)
            .copied()
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn record_unknown_groups(groups: &[String], errors: &mut Vec<String>) {
    let known = group_names();
    let unknown_groups: BTreeSet<_> = groups
        .iter()
        .filter(|g| !known.contains(g.to_lowercase().as_str()))
        .cloned()
        .collect();
    for g in unknown_groups {
        errors.push(format!("unknown group: {g}"));
    }
}

fn warn_for_missing_selective_requests(
    mode: Mode,
    actions: &[FlashAction],
    explicit_names: &BTreeSet<String>,
    available_names: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) {
    if mode != Mode::Selective {
        return;
    }
    let planned_names: BTreeSet<_> = actions
        .iter()
        .map(|a| a.partition.to_lowercase())
        .collect();
    for req in explicit_names {
        if !available_names.contains(req) && !planned_names.contains(req) {
            warnings.push(format!(
                "requested partition not found in selected layout: {req}"
            ));
        }
    }
}

fn synthesize_slot_actions_if_needed(
    selected_parts: &[ScatterPartition],
    actions: &mut Vec<FlashAction>,
) {
    synthesize_non_download_slot_actions(selected_parts, actions);
}

fn synthesize_non_download_slot_actions(
    selected_parts: &[ScatterPartition],
    actions: &mut Vec<FlashAction>,
) {
    let parts_by_name: BTreeMap<_, _> = selected_parts
        .iter()
        .map(|p| (p.name.to_lowercase(), p))
        .collect();
    let actions_by_partition: BTreeMap<_, _> = actions
        .iter()
        .filter(|a| a.action == "flash")
        .map(|a| (a.partition.to_lowercase(), a.clone()))
        .collect();
    let planned: BTreeSet<_> = actions_by_partition.keys().cloned().collect();
    let mut synthesized = Vec::new();

    for source_action in actions_by_partition.values() {
        let Some(source_slot) = source_action.slot.as_deref() else {
            continue;
        };
        let target_slot = match source_slot {
            "a" => "b",
            "b" => "a",
            _ => continue,
        };
        let target_name = format!("{}_{}", source_action.base_name, target_slot);
        if planned.contains(&target_name) {
            continue;
        }
        let Some(target_part) = parts_by_name.get(&target_name) else {
            continue;
        };
        if target_part.flashable_by_profile() {
            continue;
        }
        synthesized.push(slot_synthesized_action(source_action, target_part, source_slot));
    }

    actions.extend(synthesized);
}

fn slot_synthesized_action(
    source: &FlashAction,
    target: &ScatterPartition,
    source_slot: &str,
) -> FlashAction {
    let (image, warnings) = recheck_synthesized_image(source.image.clone(), target);
    FlashAction {
        action: source.action.clone(),
        execution_kind: source.execution_kind,
        partition: target.name.clone(),
        base_name: target.base_name(),
        slot: target.slot(),
        layout: target.layout.clone(),
        region: target.region.clone(),
        start: target.linear_start,
        start_hex: format!("{:#x}", target.linear_start),
        size: target.size,
        size_hex: format!("{:#x}", target.size),
        size_human: human_size(target.size),
        image,
        image_type: target.image_type.clone(),
        safety_class: target.safety_class(),
        reason: format!("inferred from slot {source_slot} image for slot all"),
        warnings,
    }
}

fn recheck_synthesized_image(
    image: Option<Value>,
    target: &ScatterPartition,
) -> (Option<Value>, Vec<String>) {
    let Some(mut image) = image else {
        return (None, Vec::new());
    };
    let mut warnings = Vec::new();
    if let Some(warning) = image.pointer("/path/warning").and_then(Value::as_str) {
        warnings.push(warning.to_string());
    }
    let checked = image
        .pointer("/status/checked")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !checked {
        return (Some(image), warnings);
    }

    let (status, mut status_warnings) = checked_image_status(
        image.pointer("/path/resolved_path").and_then(Value::as_str),
        image.pointer("/path/exists").and_then(Value::as_bool),
        true,
        target.size,
    );
    warnings.append(&mut status_warnings);
    image["status"] = status;
    (Some(image), warnings)
}

fn check_incomplete_slots(
    selected_parts: &[ScatterPartition],
    actions: &[FlashAction],
    allow_incomplete_slots: bool,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> BTreeMap<String, Value> {
    let mut incomplete_slots = BTreeMap::new();

    let mut by_base_available: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut by_base_planned: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for part in selected_parts {
        if let Some(slot) = part.slot() {
            by_base_available
                .entry(part.base_name())
                .or_default()
                .insert(slot);
        }
    }
    for action in actions.iter().filter(|a| a.action == "flash") {
        if let Some(slot) = &action.slot {
            by_base_planned
                .entry(action.base_name.clone())
                .or_default()
                .insert(slot.clone());
        }
    }

    for (base, available) in by_base_available {
        if !available.is_superset(&BTreeSet::from(["a".to_string(), "b".to_string()])) {
            continue;
        }
        let planned = by_base_planned.get(&base).cloned().unwrap_or_default();
        if !planned.is_empty()
            && planned != BTreeSet::from(["a".to_string(), "b".to_string()])
        {
            let available_slots: Vec<_> = available.iter().cloned().collect();
            let planned_slots: Vec<_> = planned.iter().cloned().collect();
            incomplete_slots.insert(
                base.clone(),
                json!({
                    "available_slots": available_slots,
                    "planned_slots": planned_slots,
                }),
            );
            let msg = format!(
                "slot policy both requested but only planned slots {planned_slots:?} for {base}; available slots are {available_slots:?}"
            );
            if allow_incomplete_slots {
                warnings.push(msg);
            } else {
                errors.push(msg);
            }
        }
    }
    incomplete_slots
}

// --- Action helpers ---

fn flash_action(
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

fn skipped_partition(part: &ScatterPartition, reason: &str) -> SkippedPartition {
    SkippedPartition {
        partition: part.name.clone(),
        layout: part.layout.clone(),
        region: part.region.clone(),
        reason: reason.to_string(),
        safety_class: part.safety_class(),
        file_name: part.file_name.clone(),
    }
}

// --- Summary ---

#[derive(Clone, Copy)]
struct PlanSummaryCounts {
    skipped_count: usize,
    missing_image_count: usize,
    oversized_image_count: usize,
    action_warning_count: usize,
    incomplete_slot_base_count: usize,
    warning_count: usize,
    error_count: usize,
}

const fn finalize_plan_summary(
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

#[cfg(test)]
mod tests {
    use super::*;
    // parse_scatter is used indirectly via the test helpers

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
            unknown_fields: BTreeMap::new(),
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
        let mut layouts = BTreeMap::new();
        layouts.insert(
            "EMMC".to_string(),
            vec![
                synthetic_part("boot_a", true, true, 0x400000),
                synthetic_part("boot_b", false, false, 0x400000),
                synthetic_part("dtbo_a", true, true, 0x100000),
                synthetic_part("dtbo_b", false, false, 0x100000),
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
        let mut layouts = BTreeMap::new();
        layouts.insert("EMMC".to_string(), vec![]);
        layouts.insert("UFS".to_string(), vec![synthetic_part("boot", true, true, 0x400000)]);
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

        let plan = build_flash_plan(&scatter, FlashPlanOptions::default());
        assert_eq!(plan.selected_layouts, vec!["UFS"]);
    }

    #[test]
    fn build_flash_plan_should_error_when_both_slots_are_incomplete() {
        let mut layouts = BTreeMap::new();
        layouts.insert(
            "EMMC".to_string(),
            vec![
                synthetic_part("boot_a", true, true, 0x400000),
                synthetic_part("boot_b", true, true, 0x400000),
                synthetic_part("dtbo_a", true, true, 0x100000),
                synthetic_part("dtbo_b", true, true, 0x100000),
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
        let plan = build_flash_plan(&scatter, FlashPlanOptions {
            mode: Mode::Selective,
            parts: vec!["boot_a".to_string()],
            ..FlashPlanOptions::default()
        });
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
        let plan = build_flash_plan(&scatter, FlashPlanOptions {
            mode: Mode::DryRun,
            ..FlashPlanOptions::default()
        });
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
        let plan = build_flash_plan(&scatter, FlashPlanOptions {
            mode: Mode::DryRun,
            ..FlashPlanOptions::default()
        });
        assert!(
            !plan.actions.iter().any(|a| a.partition == "userdata"),
            "dry run should skip userdata"
        );
    }
}
