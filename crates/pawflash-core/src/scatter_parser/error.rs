use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during scatter parsing and flash planning.
#[derive(Error, Debug)]
pub enum Error {
    /// The path does not point to a regular file.
    #[error("scatter path is not a file: {0}")]
    NotFile(PathBuf),

    /// File I/O failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// XML parsing failed.
    #[error("XML parse failed: {0}")]
    Xml(String),

    /// YAML parsing failed.
    #[error("YAML parse failed: {0}")]
    Yaml(String),

    /// A field value could not be parsed.
    #[error("{0}")]
    InvalidValue(String),

    /// Image basename search found ambiguous matches.
    #[error("{0}")]
    AmbiguousImage(String),
}

/// Convenience alias for `Result<T, scatter_parser::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
