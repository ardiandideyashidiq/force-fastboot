//! MediaTek scatter file parsing (XML and YAML formats).

mod helpers;
mod json;
mod xml;
mod yaml;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::scatter_parser::error::{Error, Result};
use crate::scatter_parser::parse::helpers::{
    get_first, normalize_none_string, parse_bool, parse_field_int,
};
use crate::scatter_parser::types::{ScatterFile, ScatterPartition};
use crate::scatter_parser::util::{region_family, storage_family};

// --- Re-exports ---

pub use helpers::{human_size, parse_int};
pub use json::partition_to_json;
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
    let text = decode_text(path)?;
    let text_hash = sha256_text(&text);
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let parsed = if looks_like_xml(&text) {
        xml::parse_xml_scatter(&text).map_err(|e| Error::Xml(e.to_string()))?
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

// --- Partition normalization ---

/// Infer effective storage layout from region/storage fields.
/// If region or storage indicates UFS, it's UFS; otherwise EMMC.
fn effective_layout(region: &str, storage: Option<&str>) -> String {
    if region_family(region) == "UFS"
        || storage_family(storage, None, Some(region)) == "UFS"
    {
        "UFS".to_string()
    } else {
        "EMMC".to_string()
    }
}

fn normalize_partition(
    path: &Path,
    layout: &str,
    entry: Map<String, Value>,
) -> Result<ScatterPartition> {
    let name = normalize_none_string(get_first(&entry, &["partition_name", "name"]))
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "partition without partition_name in layout {layout}: {entry:?}"
            ))
        })?;
    let file_name = normalize_none_string(get_first(&entry, &["file_name", "filename"]));
    let known = [
        "partition_index", "partition_name", "file_name", "is_download", "type",
        "linear_start_addr", "physical_start_addr", "partition_size", "region",
        "storage", "boundary_check", "is_reserved", "operation_type",
        "is_upgradable", "empty_boot_needed", "combo_partsize_check", "reserve",
    ];
    let unknown_fields = entry
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    let region = value_to_string(get_first(&entry, &["region"]))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let storage = normalize_none_string(get_first(&entry, &["storage"]));
    let ef_layout = effective_layout(&region, storage.as_deref());

    Ok(ScatterPartition {
        source: path.to_string_lossy().into_owned(),
        layout: ef_layout,
        index: normalize_none_string(get_first(&entry, &["partition_index"])),
        name,
        file_name,
        is_download: parse_bool(get_first(&entry, &["is_download"]), false),
        image_type: normalize_none_string(get_first(&entry, &["type"])),
        linear_start: parse_field_int(
            get_first(&entry, &["linear_start_addr"]),
            "linear_start_addr",
            0,
        )?,
        physical_start: parse_field_int(
            get_first(&entry, &["physical_start_addr"])
                .or_else(|| get_first(&entry, &["linear_start_addr"])),
            "physical_start_addr",
            0,
        )?,
        size: parse_field_int(
            get_first(&entry, &["partition_size"]),
            "partition_size",
            0,
        )?,
        region,
        storage,
        boundary_check: parse_bool(get_first(&entry, &["boundary_check"]), true),
        is_reserved: parse_bool(get_first(&entry, &["is_reserved"]), false),
        operation_type: normalize_none_string(get_first(&entry, &["operation_type"])),
        is_upgradable: entry
            .get("is_upgradable")
            .map(|v| parse_bool(Some(v), false)),
        empty_boot_needed: entry
            .get("empty_boot_needed")
            .map(|v| parse_bool(Some(v), false)),
        combo_partsize_check: entry
            .get("combo_partsize_check")
            .map(|v| parse_bool(Some(v), false)),
        raw: Value::Object(entry),
        unknown_fields,
    })
}

fn validate_layouts(
    layouts: &BTreeMap<String, Vec<ScatterPartition>>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    for (layout, parts) in layouts {
        let mut seen: std::collections::HashMap<
            (String, String),
            &ScatterPartition,
        > = std::collections::HashMap::new();
        for part in parts {
            if part.size < 0 {
                errors.push(format!("{layout}/{}: negative partition size", part.name));
            }
            if part.linear_start < 0 || part.physical_start < 0 {
                errors.push(format!("{layout}/{}: negative address", part.name));
            }
            let key = (part.region.clone(), part.name.to_lowercase());
            if let Some(old) = seen.get(&key) {
                let same_extent = old.linear_start == part.linear_start
                    && old.physical_start == part.physical_start
                    && old.size == part.size;
                let same_profile = old.file_name == part.file_name
                    && old.is_download == part.is_download
                    && old.operation_type == part.operation_type;
                if same_extent && same_profile {
                    warnings.push(format!(
                        "{layout}/{}/{}: exact duplicate declaration",
                        part.region, part.name
                    ));
                } else {
                    errors.push(format!(
                        "{layout}/{}/{}: ambiguous duplicate partition old={:#x}+{:#x} new={:#x}+{:#x}",
                        part.region, part.name, old.linear_start, old.size,
                        part.linear_start, part.size
                    ));
                }
            } else {
                seen.insert(key, part);
            }
        }

        let mut by_region: BTreeMap<&str, Vec<&ScatterPartition>> = BTreeMap::new();
        for part in parts {
            if part.is_reserved || !part.boundary_check || part.size == 0 {
                continue;
            }
            by_region.entry(&part.region).or_default().push(part);
        }
        for (region, mut items) in by_region {
            items.sort_by_key(|p| (p.linear_start, p.end(), p.name.clone()));
            for pair in items.windows(2) {
                let prev = pair[0];
                let cur = pair[1];
                if prev.end() > cur.linear_start {
                    errors.push(format!(
                        "{layout}/{region}: overlap {} [{:#x},{:#x}) with {} [{:#x},{:#x})",
                        prev.name, prev.linear_start, prev.end(),
                        cur.name, cur.linear_start, cur.end()
                    ));
                }
            }
        }
    }
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
