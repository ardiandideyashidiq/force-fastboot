#![warn(missing_docs)]

//! `pawflash` — MTK device flashing toolkit.
//!
//! # Modules
//!
//! - [`force_fastboot`] — force a device into fastboot mode via preloader serial
//! - [`scatter_parser`] — parse `MediaTek` scatter manifests and build flash plans
//! - [`flash`] — execute flash plans via fastboot protocol
//! - [`cli`] — CLI handlers for each subcommand

/// CLI subcommand handlers.
pub mod cli;
pub mod flash;
pub mod force_fastboot;
pub mod format;
pub mod scatter_parser;
