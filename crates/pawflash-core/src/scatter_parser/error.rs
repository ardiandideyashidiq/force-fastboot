use std::path::PathBuf;
use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

/// Errors that can occur during scatter parsing and flash planning.
#[derive(Error, Debug, Diagnostic)]
pub enum Error {
    /// The path does not point to a regular file.
    #[error("scatter path is not a file: {0}")]
    NotFile(PathBuf),

    /// File I/O failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// XML parsing failed with source context.
    #[error("XML parse failed: {detail}")]
    #[diagnostic(code("pawflash::scatter::xml"), help("the scatter file appears malformed"))]
    Xml {
        /// Human-readable error detail.
        detail: String,
        /// Full source text of the scatter file for snippet display.
        #[source_code]
        source_text: NamedSource<String>,
        /// Byte offset of the error location.
        #[label("here")]
        span: SourceSpan,
    },

    /// YAML parsing failed.
    #[error("YAML parse failed: {0}")]
    #[diagnostic(code("pawflash::scatter::yaml"), help("the scatter file appears malformed"))]
    Yaml(String),

    /// A field value could not be parsed.
    #[error("{detail}")]
    #[diagnostic(help("check the scatter file for invalid values"))]
    InvalidValue {
        /// Human-readable error detail.
        detail: String,
        /// Full source text of the scatter file (optional, set when available).
        #[source_code]
        source_text: Option<NamedSource<String>>,
        /// Byte offset of the error location (optional, set when available).
        #[label("here")]
        span: Option<SourceSpan>,
    },

    /// Image basename search found ambiguous matches.
    #[error("{0}")]
    #[diagnostic(help("multiple images match; use --firmware-dir or explicit paths"))]
    AmbiguousImage(String),
}

/// Convenience alias for `Result<T, scatter_parser::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
