#![deny(missing_docs)]

//! `MediaTek` scatter parser and flash-plan generator.
//!
//! # Overview
//!
//! This module provides:
//! - [`parse::parse_scatter`] — load and normalize a `MediaTek` scatter file
//! - [`plan::build_flash_plan`] — build a safe flash plan from a parsed scatter
//! - [`types`] — core data structures (`ScatterFile`, `ScatterPartition`, `FlashPlan`, …)
//! - [`safety`] — partition name canonicalization and safety classification
//! - [`error`] — reusable error types backed by `thiserror`
//! - [`parse::parse_int`] — parse MTK integer conventions
//! - [`parse::human_size`] — format byte sizes
//! - [`parse::resolve_image_path`] — resolve image file paths
//! - [`parse::image_magic`] — detect image type from magic bytes

/// Reusable error types backed by `thiserror`.
pub mod error;
/// Scatter file parsing (XML and YAML) and helper functions.
pub mod parse;
/// Image file path resolution.
pub mod path;
/// Flash plan builder.
pub mod plan;
/// Partition name canonicalization and safety classification.
pub mod safety;
/// Core data structures.
pub mod types;
/// Shared utility functions.
pub mod util;

pub use error::{Error, Result};
pub use parse::{human_size, image_magic, parse_int, parse_scatter};
pub use path::resolve_image_path;
pub use plan::build_flash_plan;
pub use safety::{canonical_name, safety_class, role_for_name};
pub use types::{
    FlashAction, FlashActionExecutionKind, FlashPlan, FlashPlanOptions, FlashPlanSummary,
    ImageHandling, Mode, ResolvedPath, ScatterFile, ScatterPartition, SkippedPartition,
    SlotPolicy, StorageSelect,
};
