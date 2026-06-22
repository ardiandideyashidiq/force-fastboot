use std::collections::BTreeSet;
use crate::scatter_parser::safety::{
    BOOTLOADER_CANONICAL, BOOT_CHAIN_CANONICAL, MODEM_CANONICAL, MCU_FW_CANONICAL,
    ANDROID_CANONICAL, REGIONAL_CANONICAL,
};
use crate::scatter_parser::types::{FlashPlanOptions, Mode, ScatterPartition};

pub(crate) fn mode_str(mode: Mode) -> String {
    match mode {
        Mode::DryRun => "dry-run",
        Mode::Selective => "selective",
        Mode::DirtyFlash => "dirty-flash",
    }
    .to_string()
}

pub(crate) fn storage_str(storage: crate::scatter_parser::types::StorageSelect) -> String {
    use crate::scatter_parser::types::StorageSelect;
    match storage {
        StorageSelect::Auto => "auto",
        StorageSelect::All => "all",
        StorageSelect::Ufs => "ufs",
        StorageSelect::Emmc => "emmc",
    }
    .to_string()
}

pub(crate) fn select_partition_for_mode(
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
            let by_group = super::group::part_matches_group(part, &options.groups);
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

pub(crate) fn mode_allows_partition(
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
