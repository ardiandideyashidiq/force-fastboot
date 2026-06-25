use std::collections::BTreeSet;

use crate::scatter_parser::safety::{
    ANDROID_CANONICAL, BOOTLOADER_CANONICAL, BOOT_CHAIN_CANONICAL, MCU_FW_CANONICAL,
    MODEM_CANONICAL, REGIONAL_CANONICAL,
};
use crate::scatter_parser::types::{
    FlashAction, FlashPlanOptions, Mode, ScatterPartition, StorageSelect,
};

pub(super) fn mode_str(mode: Mode) -> String {
    match mode {
        Mode::DryRun => "dry-run",
        Mode::Selective => "selective",
        Mode::DirtyFlash => "dirty-flash",
    }
    .to_string()
}

pub(super) fn storage_str(storage: StorageSelect) -> String {
    match storage {
        StorageSelect::Auto => "auto",
        StorageSelect::All => "all",
        StorageSelect::Ufs => "ufs",
        StorageSelect::Emmc => "emmc",
    }
    .to_string()
}

pub(super) fn select_partition_for_mode(
    part: &ScatterPartition,
    options: &FlashPlanOptions,
    explicit_names: &BTreeSet<String>,
) -> (bool, String) {
    match options.mode {
        Mode::DryRun | Mode::DirtyFlash => (true, format!("mode {}", mode_str(options.mode))),
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

pub(super) fn mode_allows_partition(
    part: &ScatterPartition,
    image_source: &ScatterPartition,
    mode: Mode,
    include_preloader: bool,
    clean: bool,
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
                (
                    false,
                    "not selected by scatter profile or no image".to_string(),
                )
            }
        }
        Mode::Selective => {
            if flashable {
                (true, "selected by user".to_string())
            } else {
                (
                    false,
                    "selected but not flashable by scatter profile".to_string(),
                )
            }
        }
        Mode::DirtyFlash => {
            if !flashable {
                return (
                    false,
                    "not selected by scatter profile or no image".to_string(),
                );
            }
            if clean && canonical == "userdata" {
                return (true, "allowed by --clean".to_string());
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

pub(super) fn part_matches_group(part: &ScatterPartition, groups: &[String]) -> bool {
    let canonical = part.canonical();
    groups
        .iter()
        .any(|group| group_members(group).contains(&canonical.as_str()))
}

pub(super) fn group_names() -> BTreeSet<&'static str> {
    [
        "boot",
        "bootloader",
        "avb",
        "modem",
        "mcu",
        "firmware",
        "android",
        "regional",
        "full-safe",
    ]
    .into_iter()
    .collect()
}

pub(super) fn group_members(group: &str) -> BTreeSet<&'static str> {
    match group.trim().to_lowercase().as_str() {
        "boot" => BOOT_CHAIN_CANONICAL.iter().copied().collect(),
        "bootloader" => BOOTLOADER_CANONICAL.iter().copied().collect(),
        "avb" => ["vbmeta", "vbmeta_system", "vbmeta_vendor"]
            .into_iter()
            .collect(),
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

pub(super) fn record_unknown_groups(groups: &[String], errors: &mut Vec<String>) {
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

pub(super) fn warn_for_missing_selective_requests(
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
