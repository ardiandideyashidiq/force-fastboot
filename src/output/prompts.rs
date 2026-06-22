use anyhow::{Context, Result};
use inquire::{Confirm, Select};

#[allow(clippy::missing_errors_doc)]
pub fn confirm_yes(prompt: &str) -> Result<bool> {
    Confirm::new(prompt)
        .with_default(true)
        .prompt()
        .context("confirm prompt failed")
}

#[allow(clippy::missing_errors_doc)]
pub fn confirm_no(prompt: &str) -> Result<bool> {
    Confirm::new(prompt)
        .with_default(false)
        .prompt()
        .context("confirm prompt failed")
}

#[allow(clippy::missing_errors_doc)]
pub fn select<'a>(title: &str, options: Vec<&'a str>) -> Result<&'a str> {
    Select::new(title, options)
        .prompt()
        .context("select prompt failed")
}
