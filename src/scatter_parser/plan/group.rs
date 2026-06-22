use std::collections::BTreeSet;
use crate::scatter_parser::safety::{
    BOOTLOADER_CANONICAL, BOOT_CHAIN_CANONICAL, MODEM_CANONICAL, MCU_FW_CANONICAL,
    ANDROID_CANONICAL, REGIONAL_CANONICAL,
};
use crate::scatter_parser::types::{FlashAction, Mode, ScatterPartition};

pub(crate) fn part_matches_group(part: &ScatterPartition, groups: &[String]) -> bool {
    let canonical = part.canonical();
    groups.iter().any(|group| group_members(group).contains(&canonical.as_str()))
}

pub(crate) fn group_names() -> BTreeSet<&'static str> {
    [
        "boot", "bootloader", "avb", "modem", "mcu", "firmware",
        "android", "regional", "full-safe",
    ]
    .into_iter()
    .collect()
}

pub(crate) fn group_members(group: &str) -> BTreeSet<&'static str> {
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

pub(crate) fn record_unknown_groups(groups: &[String], errors: &mut Vec<String>) {
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

pub(crate) fn warn_for_missing_selective_requests(
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
