use anyhow::{Context, Result};
use inquire::{Confirm, Select};

/// Prompt the user for a yes/no answer, defaulting to yes.
///
/// # Errors
///
/// Returns an error if the prompt cannot be displayed or the user's input
/// cannot be parsed.
pub fn confirm_yes(prompt: &str) -> Result<bool> {
    Confirm::new(prompt)
        .with_default(true)
        .prompt()
        .context("confirm prompt failed")
}

/// Prompt the user for a yes/no answer, defaulting to no.
///
/// # Errors
///
/// Returns an error if the prompt cannot be displayed or the user's input
/// cannot be parsed.
pub fn confirm_no(prompt: &str) -> Result<bool> {
    Confirm::new(prompt)
        .with_default(false)
        .prompt()
        .context("confirm prompt failed")
}

/// Prompt the user to select one option from a list.
///
/// # Errors
///
/// Returns an error if the prompt cannot be displayed or the user's input
/// cannot be parsed.
pub fn select<'a>(title: &str, options: Vec<&'a str>) -> Result<&'a str> {
    Select::new(title, options)
        .prompt()
        .context("select prompt failed")
}
