use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Storage layout selection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StorageSelect {
    /// Prefer UFS, then EMMC, then the first available layout.
    #[default]
    Auto,
    /// Include all layouts.
    All,
    /// Select UFS only.
    Ufs,
    /// Select EMMC only.
    Emmc,
}

/// Flash planning mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Reflect scatter-selected flashable partitions (no side effects).
    #[default]
    DryRun,
    /// Flash only explicitly requested partitions or groups.
    Selective,
    /// Flash safe firmware and Android partitions.
    DirtyFlash,
}

/// Image path resolution result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedPath {
    /// Original input path string.
    pub original: Option<String>,
    /// Normalized path string.
    pub normalized: Option<String>,
    /// Resolved absolute path if found.
    pub resolved_path: Option<String>,
    /// How the path was resolved.
    pub resolved_via: Option<String>,
    /// Whether the resolved path exists on disk.
    pub exists: Option<bool>,
    /// Whether the input was an absolute path.
    pub is_absolute_input: bool,
    /// The input style ("posix" or "windows").
    pub input_style: Option<String>,
    /// Whether the path contains parent reference (`..`).
    pub contains_parent_reference: bool,
    /// Whether the resolved path falls outside the package root.
    pub outside_package_root: Option<bool>,
    /// Warning message about path resolution.
    pub warning: Option<String>,
}

/// One normalized scatter partition.
#[derive(Debug, Clone, Serialize)]
pub struct ScatterPartition {
    /// Source file path.
    pub source: String,
    /// Storage layout name (e.g. "EMMC", "UFS").
    pub layout: String,
    /// Partition index string (e.g. "SYS0").
    pub index: Option<String>,
    /// Partition name (e.g. "boot", "preloader").
    pub name: String,
    /// Image file name, if any.
    pub file_name: Option<String>,
    /// Whether this partition is marked for download.
    pub is_download: bool,
    /// Image type hint (e.g. "`SV5_BL_BIN`").
    #[serde(rename = "type")]
    pub image_type: Option<String>,
    /// Linear start address in bytes.
    pub linear_start: i64,
    /// Physical start address in bytes.
    pub physical_start: i64,
    /// Partition size in bytes.
    pub size: i64,
    /// Storage region (e.g. "`EMMC_BOOT1_BOOT2`").
    pub region: String,
    /// Storage type identifier.
    pub storage: Option<String>,
    /// Whether boundary check is enabled.
    pub boundary_check: bool,
    /// Whether the partition is reserved.
    pub is_reserved: bool,
    /// Operation type hint (e.g. "BOOTLOADERS").
    pub operation_type: Option<String>,
    /// Whether the partition is upgradable.
    pub is_upgradable: Option<bool>,
    /// Whether empty boot is needed.
    pub empty_boot_needed: Option<bool>,
    /// Whether combo partition size check is enabled.
    pub combo_partsize_check: Option<bool>,
    /// Raw partition data from scatter.
    pub raw: Value,
    /// Unknown or unrecognized fields from the scatter.
    pub unknown_fields: BTreeMap<String, Value>,
}

impl ScatterPartition {
    /// End offset (`linear_start` + size).
    #[must_use]
    pub const fn end(&self) -> i64 {
        self.linear_start + self.size
    }

    /// Base partition name without slot suffix.
    #[must_use]
    pub fn base_name(&self) -> String {
        split_base_slot(&self.name).0
    }

    /// Slot suffix if present (e.g. `"_a"`, `"_b"`).
    #[must_use]
    pub fn slot(&self) -> Option<String> {
        split_base_slot(&self.name).1
    }

    /// Canonical name for role/safety matching.
    #[must_use]
    pub fn canonical(&self) -> String {
        canonical_name(&self.name)
    }

    /// Region family identifier.
    #[must_use]
    pub fn region_family(&self) -> String {
        region_family(&self.region)
    }

    /// Storage family identifier.
    #[must_use]
    pub fn storage_family(&self) -> String {
        storage_family(self.storage.as_deref(), Some(&self.layout), Some(&self.region))
    }

    /// Whether this partition is flashable by scatter profile.
    #[must_use]
    pub const fn flashable_by_profile(&self) -> bool {
        self.is_download && self.file_name.is_some() && self.size > 0
    }

    /// Safety classification for this partition.
    #[must_use]
    pub fn safety_class(&self) -> String {
        safety_class(&self.name)
    }

    /// Role label for this partition.
    #[must_use]
    pub fn role(&self) -> String {
        role_for_name(&self.name)
    }
}

/// Parsed scatter file with all layouts.
#[derive(Debug, Clone)]
pub struct ScatterFile {
    /// Path to the scatter file on disk.
    pub path: std::path::PathBuf,
    /// Source format ("xml" or "yaml").
    pub format: String,
    /// SHA-256 hash of the raw text content.
    pub text_hash: String,
    /// Platform name from scatter metadata.
    pub platform: Option<String>,
    /// Project name from scatter metadata.
    pub project: Option<String>,
    /// Raw general section from the scatter.
    pub general: Value,
    /// Partition layouts keyed by storage type name.
    pub layouts: BTreeMap<String, Vec<ScatterPartition>>,
    /// Warnings produced during parsing.
    pub warnings: Vec<String>,
    /// Errors produced during parsing.
    pub errors: Vec<String>,
}

impl ScatterFile {
    /// Return a canonical chipset label derived from scatter metadata.
    #[must_use]
    pub fn chipset(&self) -> Option<String> {
        chipset_label(self.platform.as_deref(), self.project.as_deref())
    }
}

/// Whether to include userdata in the flash plan via --clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanMode {
    /// Do not include userdata in the flash plan.
    #[default]
    No,
    /// Include userdata in the flash plan (erase and format).
    Yes,
}

/// Image verification options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageVerification {
    /// Whether to verify image file existence and size.
    pub check_images: bool,
    /// Whether to search for images by basename.
    pub image_search: bool,
}

/// Flash allowance options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Allowance {
    /// Whether to include preloader in dirty-flash mode.
    pub include_preloader: bool,
    /// Whether to allow incomplete slot pairs.
    pub allow_incomplete_slots: bool,
}

/// Flash plan options.
#[derive(Debug, Clone, Default)]
pub struct FlashPlanOptions {
    /// Flash planning mode.
    pub mode: Mode,
    /// Storage layout selection strategy.
    pub storage: StorageSelect,
    /// Explicit partition names to include.
    pub parts: Vec<String>,
    /// Partition groups to include.
    pub groups: Vec<String>,
    /// Partition names to exclude.
    pub exclude: Vec<String>,
    /// Directory containing firmware images.
    pub firmware_dir: Option<std::path::PathBuf>,
    /// Package root directory for resolving image paths.
    pub package_root: Option<std::path::PathBuf>,
    /// Image verification settings.
    pub image_verification: ImageVerification,
    /// Flash allowance settings.
    pub allowance: Allowance,
    /// Include userdata in the flash plan.
    pub clean: CleanMode,
}

/// Flash plan summary counts.
#[derive(Debug, Clone, Serialize, Default)]
pub struct FlashPlanSummary {
    /// Number of flash actions.
    pub flash_count: usize,
    /// Number of skipped partitions.
    pub skipped_count: usize,
    /// Number of actions with missing images.
    pub missing_image_count: usize,
    /// Number of actions with oversized images.
    pub oversized_image_count: usize,
    /// Total warnings across all actions.
    pub action_warning_count: usize,
    /// Number of incomplete slot base names.
    pub incomplete_slot_base_count: usize,
    /// Number of plan-level warnings.
    pub warning_count: usize,
    /// Number of plan-level errors.
    pub error_count: usize,
}

/// A flash action.
#[derive(Debug, Clone, Serialize)]
pub struct FlashAction {
    /// Action type.
    pub action: String,
    /// Full partition name.
    pub partition: String,
    /// Base partition name without slot suffix.
    pub base_name: String,
    /// Slot suffix if applicable.
    pub slot: Option<String>,
    /// Storage layout name.
    pub layout: String,
    /// Storage region name.
    pub region: String,
    /// Linear start address in bytes.
    pub start: i64,
    /// Linear start address as hex string.
    pub start_hex: String,
    /// Partition size in bytes.
    pub size: i64,
    /// Partition size as hex string.
    pub size_hex: String,
    /// Human-readable partition size.
    pub size_human: String,
    /// Resolved image information.
    pub image: Option<Value>,
    /// Scatter image type hint.
    pub image_type: Option<String>,
    /// Safety classification.
    pub safety_class: String,
    /// Reason for this action.
    pub reason: String,
    /// Per-action warnings.
    pub warnings: Vec<String>,
}

impl FlashAction {
    /// Return the resolved image path for flash actions, if available.
    #[must_use]
    pub fn image_resolved_path(&self) -> Option<&str> {
        self.image
            .as_ref()?
            .pointer("/path/resolved_path")?
            .as_str()
    }

    /// Return whether the resolved image exists, if known.
    #[must_use]
    pub fn image_exists(&self) -> Option<bool> {
        self.image.as_ref()?.pointer("/path/exists")?.as_bool()
    }
}

/// A partition omitted from a plan.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedPartition {
    /// Full partition name.
    pub partition: String,
    /// Storage layout name.
    pub layout: String,
    /// Storage region name.
    pub region: String,
    /// Reason the partition was skipped.
    pub reason: String,
    /// Safety classification.
    pub safety_class: String,
    /// Image file name, if any.
    pub file_name: Option<String>,
}

/// Planned flash operations.
#[derive(Debug, Clone, Serialize)]
pub struct FlashPlan {
    /// Effective flash mode.
    pub mode: String,
    /// Storage selection strategy used.
    pub storage_selection: String,
    /// Names of selected layouts.
    pub selected_layouts: Vec<String>,
    /// Platform chipset name from scatter metadata.
    pub platform: Option<String>,
    /// Project codename from scatter metadata.
    pub project: Option<String>,
    /// Firmware image directory.
    pub firmware_dir: Option<String>,
    /// Package root directory.
    pub package_root: Option<String>,
    /// Serialized planner options.
    pub options: Value,
    /// Plan summary counts.
    pub summary: FlashPlanSummary,
    /// Flash and wipe actions.
    pub actions: Vec<FlashAction>,
    /// Partitions skipped from the plan.
    pub skipped: Vec<SkippedPartition>,
    /// Incomplete slot bases.
    pub incomplete_slots: BTreeMap<String, Value>,
    /// Plan-level warnings.
    pub warnings: Vec<String>,
    /// Plan-level errors.
    pub errors: Vec<String>,
}

use crate::scatter_parser::safety::{canonical_name, role_for_name, safety_class};
use crate::scatter_parser::util::{split_base_slot, region_family, storage_family, chipset_label};
