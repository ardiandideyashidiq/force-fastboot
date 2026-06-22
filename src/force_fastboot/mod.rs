#![warn(missing_docs)]

//! Force a `MediaTek` device into fastboot mode by repeatedly sending the
//! `FASTBOOT` handshake over its preloader serial port.
//!
//! # Overview
//!
//! This module provides:
//! - [`fastboot`] — USB-based fastboot mode detection and device listing
//! - [`serial`] — serial port scanning, opening, and preloader waiting
//! - [`error`] — reusable error types backed by `thiserror`

/// Reusable error types for serial-port and USB operations.
pub mod error;
/// Fastboot mode detection and device listing over USB.
pub mod fastboot;
/// Serial-port scanning, opening, and preloader handshake waits.
pub mod serial;
