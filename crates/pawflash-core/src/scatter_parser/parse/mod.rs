//! MediaTek scatter file parsing (XML and YAML formats).

mod helpers;
mod normalize;
mod xml;
mod yaml;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tracing::debug;

use miette::{NamedSource, SourceSpan};

use crate::scatter_parser::error::{Error, Result};
use crate::scatter_parser::types::{ScatterFile, ScatterPartition};

use normalize::{normalize_partition, validate_layouts};

// --- Re-exports ---

pub use helpers::{human_size, parse_int};
pub(crate) use helpers::{find_general_value, scalar_json, value_to_string};

/// Parse a `MediaTek` scatter file (auto-detects XML vs YAML).
///
/// # Errors
///
/// Returns [`Error::NotFile`] for non-file paths,
/// [`Error::Io`] for I/O failures,
/// [`Error::Xml`] or [`Error::Yaml`] for parse failures.
pub fn parse_scatter(path: impl AsRef<Path>) -> Result<ScatterFile> {
    let path = path.as_ref();
    if !path.is_file() {
        return Err(Error::NotFile(path.to_path_buf()));
    }
    debug!(?path, "starting scatter parse");

    let text = decode_text(path)?;
    let text_hash = sha256_text(&text);
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let is_xml = looks_like_xml(&text);
    debug!(?path, is_xml, "scatter format detected");

    let parsed = if is_xml {
        match xml::parse_xml_scatter(&text) {
            Ok(r) => r,
            Err((detail, offset)) => {
                return Err(Error::Xml {
                    detail,
                    source_text: NamedSource::new(
                        path.display().to_string(),
                        text.clone(),
                    ),
                    span: SourceSpan::new(offset.into(), 0),
                });
            }
        }
    } else {
        yaml::parse_yaml_scatter(&text)
    };

    let mut layouts: BTreeMap<String, Vec<ScatterPartition>> = BTreeMap::new();
    for (layout, entries) in parsed.layouts {
        let norm_layout = if layout.trim().is_empty() {
            "DEFAULT".to_string()
        } else {
            layout.trim().to_string()
        };
        let mut parts = Vec::new();
        for entry in entries {
            match normalize_partition(path, &norm_layout, entry) {
                Ok(part) => parts.push(part),
                Err(err) => errors.push(format!(
                    "{norm_layout}: failed to normalize partition: {err}"
                )),
            }
        }
        layouts.insert(norm_layout, parts);
    }

    validate_layouts(&layouts, &mut warnings, &mut errors);

    Ok(ScatterFile {
        path: path.to_path_buf(),
        format: parsed.format,
        text_hash,
        platform: parsed.platform,
        project: parsed.project,
        general: parsed.general,
        layouts,
        warnings,
        errors,
    })
}

// Intermediate representation used only during parsing; fields are destructured directly.
pub(crate) struct ParsedRawScatter {
    general: Value,
    layouts: BTreeMap<String, Vec<Map<String, Value>>>,
    platform: Option<String>,
    project: Option<String>,
    format: String,
}

fn sha256_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn decode_text(path: &Path) -> Result<String> {
    let raw = fs::read(path)?;
    let candidates = [
        UTF_8.decode(&raw).0.into_owned(),
        UTF_16LE.decode(&raw).0.into_owned(),
        UTF_16BE.decode(&raw).0.into_owned(),
        raw.iter().map(|&byte| char::from(byte)).collect::<String>(),
    ];
    for text in candidates {
        if text.matches('\0').count() < std::cmp::max(1, text.len() / 20) {
            return Ok(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }
    Ok(String::from_utf8_lossy(&raw)
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}

fn looks_like_xml(text: &str) -> bool {
    let trimmed = text.trim_start_matches(['\u{feff}', '\n', '\r', '\t', ' ']);
    let bytes = trimmed.as_bytes();
    let len = bytes.len().min(300);
    (len >= 7 && bytes[..7].eq_ignore_ascii_case(b"<scatter"))
        || (len >= 5 && (bytes[..5].eq_ignore_ascii_case(b"<?xml") || bytes[..5].eq_ignore_ascii_case(b"<root")))
        || (len >= 3 && bytes[..3].eq_ignore_ascii_case(b"<da"))
}

/// Detect the kind of image by magic bytes.
#[must_use]
pub fn image_magic(path: &Path) -> Option<Value> {
    let mut file = fs::File::open(path).ok()?;
    let mut head = vec![0; 8192];
    let read = file.read(&mut head).ok()?;
    head.truncate(read);
    if head.is_empty() {
        return Some(json!({"kind": "empty"}));
    }
    let kind = if head.starts_with(b"ANDROID!") {
        "android_boot_or_recovery_image"
    } else if head.starts_with(b"AVB0") {
        "android_vbmeta_image"
    } else if head.get(..4) == Some(b"\x3a\xff\x26\xed") {
        "android_sparse_image"
    } else if head.starts_with(b"ELF") || head.starts_with(b"\x7fELF") {
        "elf"
    } else if head.len() >= 0x43a
        && matches!(&head[0x438..0x43a], b"\x53\xef" | b"\xef\x53")
    {
        "possible_ext_filesystem"
    } else if head
        .get(..1024)
        .is_some_and(|bytes| bytes.windows(8).any(|w| w == b"EFI PART"))
    {
        "gpt_or_disk_image"
    } else {
        "raw_or_unknown"
    };
    Some(json!({"kind": kind}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scatter_rejects_non_file() {
        let result = parse_scatter("/nonexistent/scatter.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a file"));
    }
}
