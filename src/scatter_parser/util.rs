//! Shared utility functions for the scatter parser.

/// Split a partition name into (`base_name`, `optional_slot`).
#[must_use]
pub(crate) fn split_base_slot(name: &str) -> (String, Option<String>) {
    let lower = name.to_lowercase();
    for slot in ["_a", "_b"] {
        if let Some(base) = lower.strip_suffix(slot) {
            if !base.is_empty() {
                return (
                    base.to_string(),
                    Some(slot.trim_start_matches('_').to_string()),
                );
            }
        }
    }
    (name.to_string(), None)
}

/// Derive region family from region string.
#[must_use]
pub(crate) fn region_family(region: &str) -> String {
    let r = region.to_uppercase();
    if r.starts_with("UFS") {
        "UFS".to_string()
    } else if r.starts_with("EMMC") || (r.contains("BOOT") && r.contains("EMMC")) {
        "EMMC".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

/// Derive storage family from multiple inputs.
#[must_use]
pub(crate) fn storage_family(
    storage: Option<&str>,
    layout: Option<&str>,
    region: Option<&str>,
) -> String {
    let s = storage.unwrap_or_default().to_uppercase();
    if s.contains("UFS") {
        "UFS".to_string()
    } else if s.contains("EMMC") || s.contains("MMC") {
        "EMMC".to_string()
    } else if layout.is_some_and(|l| matches!(l.to_uppercase().as_str(), "UFS" | "EMMC")) {
        layout.unwrap_or_default().to_uppercase()
    } else {
        region.map_or_else(|| "UNKNOWN".to_string(), region_family)
    }
}

/// Compute a canonical chipset label from platform/project.
#[must_use]
pub(crate) fn chipset_label(platform: Option<&str>, project: Option<&str>) -> Option<String> {
    normalize_chipset_value(platform).or_else(|| normalize_chipset_value(project))
}

fn normalize_chipset_value(value: Option<&str>) -> Option<String> {
    let text = value?.trim().trim_matches('"').trim_matches('\'').trim();
    if text.is_empty() {
        return None;
    }
    let stripped = text.strip_prefix('@').unwrap_or(text).trim();
    if stripped.is_empty() {
        return None;
    }
    let upper = stripped.to_uppercase();
    if matches!(upper.as_str(), "TMP" | "TEMP" | "TEMPORARY" | "UNKNOWN" | "" | "NONE" | "NULL" | "N/A" | "0" | "NA") {
        None
    } else {
        Some(stripped.to_string())
    }
}
