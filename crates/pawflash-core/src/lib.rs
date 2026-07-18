//! `pawflash` — MTK device flashing toolkit.
//!
//! # Modules
//!
//! - [`force_fastboot`] — force a device into fastboot mode via preloader serial
//! - [`scatter_parser`] — parse `MediaTek` scatter manifests and build flash plans
//! - [`flash`] — execute flash plans via fastboot protocol
//! - [`cli`] — CLI handlers for each subcommand

/// Fastboot flash execution.
pub mod flash;
/// Preloader serial fastboot mode negotiation.
pub mod force_fastboot;
/// Data partition formatting (userdata, cache, metadata).
pub mod format;

/// User-facing output formatting, status lines, and tables.
pub mod output;
/// MediaTek scatter manifest parser and flash-plan builder.
pub mod scatter_parser;
