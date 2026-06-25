//! Partition-name canonicalization and safety classification.
//!
//! These tables mirror the Python scatter parser's partition groupings and
//! determine which partitions are flashable in each mode.

use crate::scatter_parser::util::split_base_slot;

pub(crate) const BOOTLOADER_CANONICAL: &[&str] = &[
    "preloader", "lk", "loader_ext", "tee", "trustzone", "tz",
];

pub(crate) const BOOT_CHAIN_CANONICAL: &[&str] = &[
    "boot", "vendor_boot", "init_boot", "dtbo", "vbmeta",
    "vbmeta_system", "vbmeta_vendor", "recovery",
];

pub(crate) const MODEM_CANONICAL: &[&str] = &[
    "md1img", "md1dsp", "md3img", "modem", "spmfw", "dpm", "pi_img",
];

pub(crate) const MCU_FW_CANONICAL: &[&str] = &[
    "scp", "sspm", "mcupm", "gz", "tinysys", "audio_dsp", "ccu", "apu", "vcp",
];

pub(crate) const ANDROID_CANONICAL: &[&str] = &[
    "super", "system", "vendor", "product", "odm", "system_ext",
    "vendor_dlkm", "odm_dlkm", "product_dlkm",
];

pub(crate) const REGIONAL_CANONICAL: &[&str] = &[
    "logo", "tkv", "country", "cust", "oem", "csci",
];

pub(crate) const IDENTITY_CANONICAL: &[&str] = &[
    "nvram", "nvdata", "nvcfg", "protect1", "protect2",
    "protect_f", "protect_s", "persist", "proinfo", "otp",
    "sec1", "nvram_backup",
];

pub(crate) const DANGEROUS_CANONICAL: &[&str] = &[
    "pgpt", "sgpt", "gpt", "mbr", "ebr1", "ebr2", "frp",
    "seccfg", "flashinfo", "bmtpool",
];

/// Canonicalize a partition name for role/safety matching.
#[must_use]
pub fn canonical_name(name: &str) -> String {
    let (mut base, _) = split_base_slot(&name.to_lowercase());
    base = base.trim().to_string();
    if matches_numbered(&base, "tee") {
        return "tee".to_string();
    }
    if matches_numbered(&base, "lk") {
        return "lk".to_string();
    }
    if base.starts_with("preloader") {
        return "preloader".to_string();
    }
    if base.starts_with("loader_ext") {
        return "loader_ext".to_string();
    }
    // NOTE: _a/_b slot suffixes are already stripped by split_base_slot above.
    if is_numbered_vbmeta(&base) {
        if base.contains("system") {
            return "vbmeta_system".to_string();
        }
        if base.contains("vendor") {
            return "vbmeta_vendor".to_string();
        }
        return "vbmeta".to_string();
    }
    base
}

enum SafetyRole {
    IdentityOrCalibration,
    Dangerous,
    BootloaderCritical,
    BootChainOrAvb,
    ModemFirmware,
    McuFirmware,
    AndroidDynamicOrSystem,
    RegionalOrBranding,
    Unknown,
}

fn classify(canonical: &str) -> SafetyRole {
    if IDENTITY_CANONICAL.contains(&canonical) {
        SafetyRole::IdentityOrCalibration
    } else if DANGEROUS_CANONICAL.contains(&canonical) {
        SafetyRole::Dangerous
    } else if BOOTLOADER_CANONICAL.contains(&canonical) {
        SafetyRole::BootloaderCritical
    } else if BOOT_CHAIN_CANONICAL.contains(&canonical) {
        SafetyRole::BootChainOrAvb
    } else if MODEM_CANONICAL.contains(&canonical) {
        SafetyRole::ModemFirmware
    } else if MCU_FW_CANONICAL.contains(&canonical) {
        SafetyRole::McuFirmware
    } else if ANDROID_CANONICAL.contains(&canonical) {
        SafetyRole::AndroidDynamicOrSystem
    } else if REGIONAL_CANONICAL.contains(&canonical) {
        SafetyRole::RegionalOrBranding
    } else {
        SafetyRole::Unknown
    }
}

/// Return a safety class for a partition name.
#[must_use]
pub fn safety_class(name: &str) -> String {
    let canonical = canonical_name(name);
    match classify(&canonical) {
        SafetyRole::Unknown => {
            // Fallback: extended heuristic patterns
            if matches!(
                canonical.as_str(),
                "super" | "system_ext" | "vendor_dlkm" | "odm_dlkm"
                    | "my_product" | "my_region" | "product" | "vendor"
                    | "odm" | "cache" | "metadata"
            ) || canonical.starts_with("system")
                || canonical.starts_with("product")
                || canonical.starts_with("vendor")
                || canonical.starts_with("odm")
            {
                "android_system"
            } else if canonical.contains("vbmeta")
                || canonical.contains("boot")
                || canonical.contains("dtbo")
                || canonical.contains("recovery")
                || canonical.contains("init_boot")
            {
                "boot_critical"
            } else if canonical.contains("logo")
                || canonical.contains("splash")
                || canonical.contains("cust")
            {
                "regional"
            } else if canonical.contains("modem")
                || canonical.contains("radio")
                || canonical.contains("dsp")
                || canonical.ends_with("_fw")
            {
                "firmware"
            } else {
                "unknown"
            }
        }
        SafetyRole::BootloaderCritical => "bootloader_critical",
        SafetyRole::BootChainOrAvb => "boot_critical",
        SafetyRole::ModemFirmware | SafetyRole::McuFirmware => "firmware",
        SafetyRole::AndroidDynamicOrSystem => "android_system",
        SafetyRole::RegionalOrBranding => "regional",
        SafetyRole::IdentityOrCalibration => "identity_or_calibration",
        SafetyRole::Dangerous => "dangerous",
    }
    .to_string()
}

/// Role label for a partition name (more specific than `safety_class`).
#[must_use]
pub fn role_for_name(name: &str) -> String {
    let canonical = canonical_name(name);
    match classify(&canonical) {
        SafetyRole::IdentityOrCalibration => "identity_or_calibration",
        SafetyRole::Dangerous => "dangerous",
        SafetyRole::BootloaderCritical => "bootloader_critical",
        SafetyRole::BootChainOrAvb => "boot_chain_or_avb",
        SafetyRole::ModemFirmware => "modem_firmware",
        SafetyRole::McuFirmware => "mcu_firmware",
        SafetyRole::AndroidDynamicOrSystem => "android_dynamic_or_system",
        SafetyRole::RegionalOrBranding => "regional_or_branding",
        SafetyRole::Unknown => "unknown",
    }
    .to_string()
}

fn matches_numbered(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 1
        && value.starts_with(prefix)
        && matches!(value.as_bytes().last(), Some(b'1' | b'2'))
}

fn is_numbered_vbmeta(value: &str) -> bool {
    let Some(last) = value.as_bytes().last() else {
        return false;
    };
    if !matches!(last, b'1' | b'2') {
        return false;
    }
    matches!(
        &value[..value.len() - 1],
        "vbmeta" | "vbmeta_system" | "vbmeta_vendor"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> String {
        canonical_name(name)
    }

    fn sc(name: &str) -> String {
        safety_class(name)
    }

    #[test]
    fn canonical_name_should_pass_through_boot() {
        assert_eq!(c("boot"), "boot");
    }

    #[test]
    fn canonical_name_should_pass_through_preloader() {
        assert_eq!(c("preloader"), "preloader");
    }

    #[test]
    fn canonical_name_should_collapse_numbered_tee() {
        assert_eq!(c("tee1"), "tee");
        assert_eq!(c("tee2"), "tee");
    }

    #[test]
    fn canonical_name_should_collapse_numbered_lk() {
        assert_eq!(c("lk1"), "lk");
        assert_eq!(c("lk2"), "lk");
    }

    #[test]
    fn canonical_name_should_collapse_numbered_vbmeta() {
        assert_eq!(c("vbmeta1"), "vbmeta");
        assert_eq!(c("vbmeta_system2"), "vbmeta_system");
        assert_eq!(c("vbmeta_vendor1"), "vbmeta_vendor");
    }

    #[test]
    fn canonical_name_should_strip_slot_suffix() {
        assert_eq!(c("boot_a"), "boot");
        assert_eq!(c("boot_b"), "boot");
    }

    #[test]
    fn safety_class_should_classify_nvram_as_identity() {
        assert_eq!(sc("nvram"), "identity_or_calibration");
    }

    #[test]
    fn safety_class_should_classify_userdata_as_unknown() {
        assert_eq!(sc("userdata"), "unknown");
    }

    #[test]
    fn safety_class_should_classify_gpt_as_dangerous() {
        assert_eq!(sc("gpt"), "dangerous");
    }

    #[test]
    fn safety_class_should_classify_preloader_as_bootloader_critical() {
        assert_eq!(sc("preloader"), "bootloader_critical");
    }

    #[test]
    fn safety_class_should_classify_boot_as_boot_critical() {
        assert_eq!(sc("boot"), "boot_critical");
    }

    #[test]
    fn safety_class_should_classify_md1img_as_firmware() {
        assert_eq!(sc("md1img"), "firmware");
    }

    #[test]
    fn safety_class_should_classify_super_as_android_system() {
        assert_eq!(sc("super"), "android_system");
    }

    #[test]
    fn safety_class_should_classify_logo_as_regional() {
        assert_eq!(sc("logo"), "regional");
    }

    #[test]
    fn safety_class_should_return_unknown_for_unmapped() {
        assert_eq!(sc("foobar"), "unknown");
        assert_eq!(sc("_dummy_"), "unknown");
    }

    #[test]
    fn role_for_name_should_classify_preloader_as_bootloader() {
        assert_eq!(role_for_name("preloader"), "bootloader_critical");
    }

    #[test]
    fn role_for_name_should_classify_boot_as_boot_chain() {
        assert_eq!(role_for_name("boot"), "boot_chain_or_avb");
    }

    #[test]
    fn role_for_name_should_classify_md1img_as_modem() {
        assert_eq!(role_for_name("md1img"), "modem_firmware");
    }

    #[test]
    fn role_for_name_should_classify_scp_as_mcu() {
        assert_eq!(role_for_name("scp"), "mcu_firmware");
    }

    #[test]
    fn role_for_name_should_classify_system_as_android() {
        assert_eq!(role_for_name("system"), "android_dynamic_or_system");
    }

    #[test]
    fn role_for_name_should_classify_cust_as_regional() {
        assert_eq!(role_for_name("cust"), "regional_or_branding");
    }
}
