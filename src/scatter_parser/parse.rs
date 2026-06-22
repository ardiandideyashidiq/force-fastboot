//! MediaTek scatter file parsing (XML and YAML formats).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8};
use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};
use serde_yaml;
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::scatter_parser::error::{Error, Result};
use crate::scatter_parser::types::{ResolvedPath, ScatterFile, ScatterPartition, region_family};

const NONE_TOKENS: &[&str] = &["", "NONE", "NULL", "N/A", "NA", "0"];

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
        parse_xml_scatter(&text).map_err(|e| Error::Xml(e.to_string()))?
    } else {
        parse_yaml_scatter(&text)
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
#[expect(dead_code)]
struct ParsedRawScatter {
    general: Value,
    layouts: BTreeMap<String, Vec<Map<String, Value>>>,
    warnings: Vec<String>,
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
    (len >= 5 && bytes[..5].eq_ignore_ascii_case(b"<?xml"))
        || (len >= 5 && bytes[..5].eq_ignore_ascii_case(b"<root"))
        || (len >= 7 && bytes[..7].eq_ignore_ascii_case(b"<scatter"))
        || (len >= 3 && bytes[..3].eq_ignore_ascii_case(b"<da"))
}

// --- XML parsing ---

#[derive(Debug, Clone, Default)]
struct XmlNode {
    tag: String,
    attrs: Map<String, Value>,
    text: String,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn descendants(&self) -> Vec<&XmlNode> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.descendants());
        }
        out
    }
}

fn parse_xml_scatter(text: &str) -> Result<ParsedRawScatter> {
    let root = parse_xml_node(text).map_err(Error::Xml)?;

    if matches!(
        root.tag.to_lowercase().as_str(),
        "scatter" | "checksum" | "scatter_checksum"
    ) && !root
        .descendants()
        .iter()
        .any(|node| node.tag == "partition_index")
    {
        return Ok(ParsedRawScatter {
            general: json!({}),
            layouts: BTreeMap::new(),
            warnings: Vec::new(),
            platform: None,
            project: None,
            format: "checksum_xml".to_string(),
        });
    }

    let mut general = Map::new();
    if let Some(general_node) = root
        .descendants()
        .into_iter()
        .find(|node| node.tag == "general")
    {
        general.extend(xml_children_dict(general_node));
        for (key, value) in &general_node.attrs {
            general
                .entry(format!("@{key}"))
                .or_insert_with(|| value.clone());
        }
    }
    for (key, value) in &root.attrs {
        general.entry(key.clone()).or_insert_with(|| value.clone());
    }

    let mut layouts: BTreeMap<String, Vec<Map<String, Value>>> = BTreeMap::new();
    for storage_node in root
        .descendants()
        .into_iter()
        .filter(|node| node.tag == "storage_type")
    {
        let layout = value_to_string(
            storage_node
                .attrs
                .get("name")
                .or_else(|| storage_node.attrs.get("value")),
        )
        .or_else(|| {
            let text = storage_node.text.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .unwrap_or_else(|| "UNKNOWN".to_string());
        for part_node in storage_node
            .descendants()
            .into_iter()
            .filter(|node| node.tag == "partition_index")
        {
            let mut entry = xml_children_dict(part_node);
            let index = value_to_string(
                part_node
                    .attrs
                    .get("name")
                    .or_else(|| part_node.attrs.get("value")),
            )
            .or_else(|| value_to_string(entry.get("partition_index")));
            if let Some(index) = index {
                entry.insert("partition_index".to_string(), Value::String(index));
            }
            layouts.entry(layout.clone()).or_default().push(entry);
        }
    }

    if layouts.is_empty() {
        let direct_parts = root
            .children
            .iter()
            .filter(|node| node.tag == "partition_index")
            .map(|node| {
                let mut entry = xml_children_dict(node);
                if let Some(index) =
                    value_to_string(node.attrs.get("name").or_else(|| node.attrs.get("value")))
                {
                    entry.insert("partition_index".to_string(), Value::String(index));
                }
                entry
            })
            .collect::<Vec<_>>();
        if !direct_parts.is_empty() {
            let joined = direct_parts
                .iter()
                .map(|entry| {
                    format!(
                        "{} {}",
                        value_to_string(entry.get("storage")).unwrap_or_default(),
                        value_to_string(entry.get("region")).unwrap_or_default()
                    )
                })
                .collect::<String>()
                .to_uppercase();
            layouts.insert(
                if joined.contains("UFS") { "UFS" } else { "EMMC" }.to_string(),
                direct_parts,
            );
        }
    }

    if layouts.is_empty() {
        let all_parts = root
            .descendants()
            .into_iter()
            .filter(|node| node.tag == "partition_index")
            .map(|node| {
                let mut entry = xml_children_dict(node);
                if let Some(index) =
                    value_to_string(node.attrs.get("name").or_else(|| node.attrs.get("value")))
                {
                    entry.insert("partition_index".to_string(), Value::String(index));
                }
                entry
            })
            .collect::<Vec<_>>();
        if !all_parts.is_empty() {
            layouts.insert("DEFAULT".to_string(), all_parts);
        }
    }

    let general_value = Value::Object(general);
    let platform = find_general_value(&general_value, "platform");
    let project = find_general_value(&general_value, "project");
    Ok(ParsedRawScatter {
        general: general_value,
        layouts,
        warnings: Vec::new(),
        platform,
        project,
        format: "xml".to_string(),
    })
}

fn parse_xml_node(text: &str) -> std::result::Result<XmlNode, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<XmlNode> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(start)) => {
                let tag = strip_ns(
                    std::str::from_utf8(start.name().as_ref()).unwrap_or_default(),
                );
                let mut attrs = Map::new();
                for attr in start.attributes().flatten() {
                    let key = strip_ns(
                        std::str::from_utf8(attr.key.as_ref()).unwrap_or_default(),
                    );
                    let value = attr
                        .unescape_value()
                        .map_or(Value::Null, |v| scalar_json(v.as_ref()));
                    attrs.insert(key, value);
                }
                stack.push(XmlNode {
                    tag,
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(empty)) => {
                let tag = strip_ns(
                    std::str::from_utf8(empty.name().as_ref()).unwrap_or_default(),
                );
                let mut attrs = Map::new();
                for attr in empty.attributes().flatten() {
                    let key = strip_ns(
                        std::str::from_utf8(attr.key.as_ref()).unwrap_or_default(),
                    );
                    let value = attr
                        .unescape_value()
                        .map_or(Value::Null, |v| scalar_json(v.as_ref()));
                    attrs.insert(key, value);
                }
                let node = XmlNode {
                    tag,
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    return Ok(node);
                }
            }
            Ok(Event::Text(event)) => {
                if let Some(node) = stack.last_mut() {
                    node.text
                        .push_str(&String::from_utf8_lossy(event.as_ref()));
                }
            }
            Ok(Event::End(_)) => {
                let Some(node) = stack.pop() else {
                    return Err("unexpected closing tag".to_string());
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    return Ok(node);
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
        buf.clear();
    }
    Err("empty XML document".to_string())
}

fn xml_children_dict(node: &XmlNode) -> Map<String, Value> {
    let mut out = Map::new();
    for child in &node.children {
        let value = xml_value(child);
        match out.get_mut(&child.tag) {
            Some(Value::Array(items)) => items.push(value),
            Some(existing) => {
                let old = std::mem::take(existing);
                *existing = Value::Array(vec![old, value]);
            }
            None => {
                out.insert(child.tag.clone(), value);
            }
        }
    }
    for (key, value) in &node.attrs {
        out.entry(key.clone()).or_insert_with(|| value.clone());
    }
    out
}

fn xml_value(node: &XmlNode) -> Value {
    if !node.children.is_empty() {
        let mut map = xml_children_dict(node);
        let text = node.text.trim();
        if !text.is_empty() {
            map.entry("#text".to_string())
                .or_insert_with(|| scalar_json(text));
        }
        return Value::Object(map);
    }

    for key in ["value", "name"] {
        if let Some(value) = node.attrs.get(key) {
            return value.clone();
        }
    }
    let text = node.text.trim();
    if !text.is_empty() {
        return scalar_json(text);
    }
    if !node.attrs.is_empty() {
        return Value::Object(node.attrs.clone());
    }
    Value::Null
}

// --- YAML parsing ---

fn parse_yaml_scatter(text: &str) -> ParsedRawScatter {
    let records = if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(text) {
        if let Ok(json_value) = serde_json::to_value(value) {
            match json_value {
                Value::Array(items) => items
                    .into_iter()
                    .filter_map(|item| item.as_object().cloned())
                    .collect(),
                Value::Object(map) => vec![map],
                _ => Vec::new(),
            }
        } else {
            loose_yaml_records(text)
        }
    } else {
        loose_yaml_records(text)
    };

    let mut general = Map::new();
    let mut layouts: BTreeMap<String, Vec<Map<String, Value>>> = BTreeMap::new();
    let mut warnings = Vec::new();

    for rec in records {
        if rec.contains_key("storage_type") && rec.contains_key("description") {
            let layout = value_to_string(rec.get("storage_type"))
                .unwrap_or_else(|| "UNKNOWN".to_string());
            if let Some(Value::Array(items)) = rec.get("description") {
                for item in items.iter().filter_map(Value::as_object) {
                    if item.contains_key("general")
                        || item.contains_key("config_version")
                        || item.contains_key("platform")
                        || item.contains_key("project")
                    {
                        if general.is_empty() {
                            general.extend(item.clone());
                        } else {
                            general
                                .entry("layout_general")
                                .or_insert_with(|| json!({}));
                        }
                    }
                    if item.contains_key("partition_name")
                        || item.contains_key("partition_index")
                    {
                        layouts.entry(layout.clone()).or_default().push(item.clone());
                    }
                }
            }
            continue;
        }

        if rec.contains_key("general")
            || rec.contains_key("config_version")
            || rec.contains_key("platform")
            || rec.contains_key("project")
        {
            for (key, value) in rec {
                general.entry(key).or_insert(value);
            }
            continue;
        }

        if rec.contains_key("partition_name") || rec.contains_key("partition_index") {
            let layout = value_to_string(
                rec.get("storage_type")
                    .or_else(|| rec.get("layout"))
                    .or_else(|| rec.get("storage")),
            )
            .unwrap_or_else(|| "DEFAULT".to_string());
            layouts.entry(layout).or_default().push(rec);
        }
    }

    if layouts.is_empty() && !general.is_empty() {
        warnings.push("no partition entries found in YAML-style scatter".to_string());
    }

    let general_value = Value::Object(general);
    let platform = find_general_value(&general_value, "platform");
    let project = find_general_value(&general_value, "project");
    ParsedRawScatter {
        general: general_value,
        layouts,
        warnings,
        platform,
        project,
        format: "yaml".to_string(),
    }
}

fn loose_yaml_records(text: &str) -> Vec<Map<String, Value>> {
    let mut records = Vec::new();
    let mut current: Option<Map<String, Value>> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('-') {
            if let Some(record) = current.take().filter(|r| !r.is_empty()) {
                records.push(record);
            }
            current = Some(Map::new());
            let trimmed = rest.trim();
            if let Some((key, value)) = trimmed.split_once(':') {
                let k = key.trim();
                if !k.is_empty() {
                    if let Some(r) = current.as_mut() {
                        r.insert(k.to_string(), scalar_json(value.trim()));
                    }
                }
            }
            continue;
        }
        let Some(record) = current.as_mut() else {
            continue;
        };
        if let Some((key, value)) = line.split_once(':') {
            let k = key.trim();
            if !k.is_empty() {
                record.insert(k.to_string(), scalar_json(value.trim()));
            }
        }
    }
    if let Some(record) = current.filter(|r| !r.is_empty()) {
        records.push(record);
    }
    records
}

// --- Partition normalization ---

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

    Ok(ScatterPartition {
        source: path.to_string_lossy().into_owned(),
        layout: layout.to_string(),
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
        region: value_to_string(get_first(&entry, &["region"]))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "UNKNOWN".to_string()),
        storage: normalize_none_string(get_first(&entry, &["storage"])),
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
            let rf = region_family(&part.region);
            let sf = part.storage_family();
            if matches!(layout.to_uppercase().as_str(), "UFS" | "EMMC")
                && matches!(rf.as_str(), "UFS" | "EMMC")
                && layout.to_uppercase() != rf
            {
                warnings.push(format!(
                    "{layout}/{}: layout is {layout} but region family is {rf}",
                    part.name
                ));
            }
            if matches!(layout.to_uppercase().as_str(), "UFS" | "EMMC")
                && matches!(sf.as_str(), "UFS" | "EMMC")
                && layout.to_uppercase() != sf
            {
                warnings.push(format!(
                    "{layout}/{}: layout is {layout} but storage family is {sf}",
                    part.name
                ));
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

// --- Image path resolution ---

/// Resolve an image file path from a scatter partition's `file_name`.
#[must_use]
pub fn resolve_image_path(
    file_name: Option<&str>,
    scatter_dir: Option<&Path>,
    firmware_dir: Option<&Path>,
    package_root: Option<&Path>,
    image_search: bool,
) -> ResolvedPath {
    let Some(original) = file_name else {
        return ResolvedPath {
            original: None, normalized: None, resolved_path: None,
            resolved_via: None, exists: None, is_absolute_input: false,
            input_style: None, contains_parent_reference: false,
            outside_package_root: None, warning: None,
        };
    };
    let normalized = normalize_path_display(original);
    let contains_parent = mixed_path_parts(&normalized)
        .iter()
        .any(|part| part == "..");
    let absolute_input =
        is_windows_absolute(original) || normalized.starts_with('/');
    let input_style = if original.contains('\\') || is_windows_absolute(original) {
        "windows"
    } else {
        "posix"
    };

    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
    if normalized.starts_with('/') {
        candidates.push(("absolute", PathBuf::from(&normalized)));
    } else if is_windows_absolute(original) {
        candidates.push(("windows_absolute", PathBuf::from(original)));
        let stripped = mixed_parts_path(original);
        if let Some(fd) = firmware_dir {
            candidates.push(("firmware_dir_windows_stripped", fd.join(&stripped)));
        }
        if let Some(sd) = scatter_dir {
            candidates.push((
                "scatter_relative_windows_stripped",
                sd.join(&stripped),
            ));
        }
    } else {
        let rel = mixed_parts_path(&normalized);
        if let Some(fd) = firmware_dir {
            candidates.push(("firmware_dir_relative", fd.join(&rel)));
        }
        if let Some(sd) = scatter_dir {
            candidates.push(("scatter_relative", sd.join(&rel)));
        }
    }

    let mut warning: Option<String> = None;
    for &(via, ref candidate) in &candidates {
        let candidate = absolutize(candidate);
        let outside =
            package_root.as_ref().map(|root| !is_within(&candidate, root));
        if outside == Some(true) {
            warning = Some(format!(
                "resolved image path is outside package_root: {}",
                candidate.display()
            ));
            continue;
        }
        if candidate.exists() {
            return resolved_path_result(ResolvedPathParts {
                original,
                normalized: &normalized,
                resolved_path: Some(candidate),
                resolved_via: Some(via),
                exists: Some(true),
                is_absolute_input: absolute_input,
                input_style,
                contains_parent_reference: contains_parent,
                outside_package_root: outside,
                warning,
            });
        }
    }

    let first_allowed = candidates.iter().find_map(|&(via, ref candidate)| {
        let candidate = absolutize(candidate);
        let outside =
            package_root.as_ref().map(|root| !is_within(&candidate, root));
        (outside != Some(true)).then_some((via, candidate, outside))
    });

    if image_search {
        let mut seen = std::collections::BTreeSet::new();
        for root in [firmware_dir, scatter_dir].into_iter().flatten() {
            let root = absolutize(root);
            if !seen.insert(root.clone()) {
                continue;
            }
            let basename = Path::new(&normalized)
                .file_name()
                .unwrap_or_else(|| OsStr::new(&normalized));
            match unique_basename_search(&root, basename) {
                Ok(Some(found)) => {
                    let outside = package_root
                        .as_ref()
                        .map(|pr| !is_within(&found, pr));
                    if outside == Some(true) {
                        warning = Some(format!(
                            "image-search result outside package_root: {}",
                            found.display()
                        ));
                        continue;
                    }
                    return resolved_path_result(ResolvedPathParts {
                        original,
                        normalized: &normalized,
                        resolved_path: Some(found),
                        resolved_via: Some("image_search_unique_basename"),
                        exists: Some(true),
                        is_absolute_input: absolute_input,
                        input_style,
                        contains_parent_reference: contains_parent,
                        outside_package_root: outside,
                        warning,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    warning = Some(err);
                    break;
                }
            }
        }
    }

    if let Some((via, candidate, outside)) = first_allowed {
        return resolved_path_result(ResolvedPathParts {
            original,
            normalized: &normalized,
            resolved_path: Some(candidate),
            resolved_via: Some(via),
            exists: Some(false),
            is_absolute_input: absolute_input,
            input_style,
            contains_parent_reference: contains_parent,
            outside_package_root: outside,
            warning,
        });
    }
    resolved_path_result(ResolvedPathParts {
        original,
        normalized: &normalized,
        resolved_path: None,
        resolved_via: None,
        exists: Some(false),
        is_absolute_input: absolute_input,
        input_style,
        contains_parent_reference: contains_parent,
        outside_package_root: package_root.as_ref().map(|_| true),
        warning: warning.or_else(|| Some("no allowed image path candidate".to_string())),
    })
}

struct ResolvedPathParts<'a> {
    original: &'a str,
    normalized: &'a str,
    resolved_path: Option<PathBuf>,
    resolved_via: Option<&'a str>,
    exists: Option<bool>,
    is_absolute_input: bool,
    input_style: &'a str,
    contains_parent_reference: bool,
    outside_package_root: Option<bool>,
    warning: Option<String>,
}

fn resolved_path_result(parts: ResolvedPathParts<'_>) -> ResolvedPath {
    ResolvedPath {
        original: Some(parts.original.to_string()),
        normalized: Some(parts.normalized.to_string()),
        resolved_path: parts
            .resolved_path
            .map(|p| p.to_string_lossy().into_owned()),
        resolved_via: parts.resolved_via.map(ToString::to_string),
        exists: parts.exists,
        is_absolute_input: parts.is_absolute_input,
        input_style: Some(parts.input_style.to_string()),
        contains_parent_reference: parts.contains_parent_reference,
        outside_package_root: parts.outside_package_root,
        warning: parts.warning,
    }
}

fn unique_basename_search(
    root: &Path,
    basename: &OsStr,
) -> std::result::Result<Option<PathBuf>, String> {
    let mut stack = vec![root.to_path_buf()];
    let mut first_match: Option<PathBuf> = None;
    while let Some(path) = stack.pop() {
        let entries =
            fs::read_dir(&path).map_err(|e| format!("image-search failed under {}: {e}", root.display()))?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if entry_path.file_name() == Some(basename) {
                let entry_path = absolutize(&entry_path);
                if let Some(first) = &first_match {
                    return Err(format!(
                        "ambiguous image basename {:?}: {}, {}",
                        basename,
                        first.display(),
                        entry_path.display()
                    ));
                }
                first_match = Some(entry_path);
            }
        }
    }
    Ok(first_match)
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

/// Serialize a `ScatterPartition` (and its resolved image) to JSON.
#[must_use]
pub fn partition_to_json(
    part: &ScatterPartition,
    scatter_dir: Option<&Path>,
    firmware_dir: Option<&Path>,
    package_root: Option<&Path>,
    check_images: bool,
    image_search: bool,
) -> Value {
    let resolved = resolve_image_path(
        part.file_name.as_deref(),
        scatter_dir,
        firmware_dir,
        package_root,
        image_search,
    );
    let mut image_status = json!({
        "checked": check_images,
        "exists": resolved.exists,
        "size_bytes": null,
        "size_human": null,
        "fits_partition": null,
        "magic": null,
    });
    if check_images {
        if let Some(path) = resolved
            .resolved_path
            .as_deref()
            .filter(|_| resolved.exists == Some(true))
        {
            if let Ok(meta) = fs::metadata(path) {
                if let Ok(size) = i64::try_from(meta.len()) {
                    image_status["size_bytes"] = json!(size);
                    image_status["size_human"] = json!(human_size(size));
                    image_status["fits_partition"] = json!(size <= part.size);
                    image_status["magic"] = json!(image_magic(Path::new(path)));
                }
            }
        }
    }
    json!({
        "name": part.name,
        "index": part.index,
        "layout": part.layout,
        "region": part.region,
        "size": part.size,
        "file_name": part.file_name,
        "is_download": part.is_download,
        "image_type": part.image_type,
        "safety_class": part.safety_class(),
        "image": {
            "basename": part.file_name.as_ref().and_then(|n| Path::new(n).file_name()).map(|n| n.to_string_lossy().into_owned()),
            "path": resolved,
            "status": image_status,
        },
        "raw": part.raw,
    })
}

// --- Scalar helper functions ---

/// Parse an integer using MTK scatter conventions (decimal, `0x` hex, `h`-suffix).
///
/// # Errors
///
/// Returns [`Error::InvalidValue`] when the string cannot be parsed.
pub fn parse_int(value: &str, field_name: &str) -> Result<i64> {
    let mut s = value.trim().replace('_', "");
    if s.is_empty() {
        return Err(Error::InvalidValue(format!("empty {field_name}")));
    }
    let sign = if let Some(rest) = s.strip_prefix('-') {
        s = rest.to_string();
        -1
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
        1
    } else {
        1
    };

    let parsed = if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(rest, 16)
    } else if let Some(rest) = s.strip_suffix('h').or_else(|| s.strip_suffix('H')) {
        i64::from_str_radix(rest, 16)
    } else if s.chars().all(|c| c.is_ascii_digit()) {
        s.parse::<i64>()
    } else if s.chars().all(|c| c.is_ascii_hexdigit())
        && s.chars().any(|c| c.is_ascii_hexdigit() && c.is_ascii_alphabetic())
    {
        i64::from_str_radix(&s, 16)
    } else {
        return Err(Error::InvalidValue(format!(
            "invalid {field_name}: {value}",
        )));
    };
    parsed.map(|n| n * sign).map_err(|_| {
        Error::InvalidValue(format!("invalid {field_name}: {value}"))
    })
}

/// Format byte sizes like the Python parser.
// Acceptable precision for partition sizes — real values cap at ~TiB, well within f64's 2⁵³.
#[expect(clippy::cast_precision_loss)]
#[expect(clippy::cast_sign_loss)]
#[must_use]
pub fn human_size(num: i64) -> String {
    let mut n = num as f64;
    for unit in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if n.abs() < 1024.0 || unit == "TiB" {
            if unit == "B" {
                return format!("{} B", n as i64);
            }
            return format!("{n:.2} {unit}");
        }
        n /= 1024.0;
    }
    format!("{num} B")
}

// --- Internal helpers ---

fn scalar_json(value: &str) -> Value {
    let s = value.trim();
    if s.is_empty() {
        return Value::String(String::new());
    }
    match s.to_lowercase().as_str() {
        "true" | "yes" => return Value::Bool(true),
        "false" | "no" => return Value::Bool(false),
        _ => {}
    }
    parse_int(s, "scalar").map_or_else(
        |_| Value::String(s.to_string()),
        |num| Value::Number(num.into()),
    )
}

fn value_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

fn parse_bool(value: Option<&Value>, default: bool) -> bool {
    match value {
        None | Some(Value::Null) => default,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or_default() != 0,
        Some(v) => match value_to_string(Some(v))
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str()
        {
            "true" | "1" | "yes" | "y" | "on" => true,
            "false" | "0" | "no" | "n" | "off" => false,
            _ => default,
        },
    }
}

fn parse_field_int(
    value: Option<&Value>,
    field_name: &str,
    default: i64,
) -> Result<i64> {
    match value {
        Some(Value::Number(n)) => n.as_i64().ok_or_else(|| {
            Error::InvalidValue(format!("invalid {field_name}: {n}"))
        }),
        Some(Value::Bool(b)) => Ok(i64::from(*b)),
        Some(v) => parse_int(
            &value_to_string(Some(v)).unwrap_or_default(),
            field_name,
        ),
        None => Ok(default),
    }
}

fn normalize_none_string(value: Option<&Value>) -> Option<String> {
    let text = value_to_string(value)?
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if text.is_empty() {
        return None;
    }
    let text_upper = text.trim().to_uppercase();
    if NONE_TOKENS.contains(&text_upper.as_str()) {
        return None;
    }
    let normalized = text.replace('\\', "/");
    let last = normalized.rsplit('/').next().unwrap_or_default().trim().to_uppercase();
    if NONE_TOKENS.contains(&last.as_str())
    {
        None
    } else {
        Some(text)
    }
}

fn get_first<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| map.get(*key))
}

fn find_general_value(general: &Value, wanted: &str) -> Option<String> {
    let wanted = wanted.to_lowercase();
    if !general.is_object() {
        return None;
    }
    let mut stack = vec![general];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if key.to_lowercase().trim_start_matches('@') == wanted
                        && !matches!(value, Value::Array(_) | Value::Object(_))
                    {
                        if let Some(v) = normalize_none_string(Some(value)) {
                            return Some(v);
                        }
                    }
                }
                for child in map.values().rev() {
                    if child.is_object() || child.is_array() {
                        stack.push(child);
                    }
                }
            }
            Value::Array(items) => {
                for child in items.iter().rev() {
                    if child.is_object() || child.is_array() {
                        stack.push(child);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_path_display(value: &str) -> String {
    value.replace('\\', "/")
}

fn mixed_path_parts(path_text: &str) -> Vec<String> {
    let value = path_text
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('\\', "/");
    let value = if value.len() >= 3
        && value.as_bytes()[1] == b':'
        && value.as_bytes()[2] == b'/'
    {
        value[3..].to_string()
    } else {
        value
    };
    value
        .trim_start_matches('/')
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .map(ToString::to_string)
        .collect()
}

fn mixed_parts_path(path_text: &str) -> PathBuf {
    mixed_path_parts(path_text).into_iter().collect()
}

fn is_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
        && bytes[0].is_ascii_alphabetic()
}

fn is_within(path: &Path, root: &Path) -> bool {
    let path = absolutize(path);
    let root = absolutize(root);
    path.starts_with(root)
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_components(path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|err| {
            warn!(%err, "failed to get current directory, using '.'");
            PathBuf::from(".")
        });
        normalize_components(&cwd.join(path))
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn strip_ns(tag: &str) -> String {
    tag.rsplit('}').next().unwrap_or(tag).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_should_accept_decimal() {
        assert_eq!(parse_int("1234", "test").unwrap(), 1234);
    }

    #[test]
    fn parse_int_should_accept_0x_hex() {
        assert_eq!(parse_int("0x1000", "test").unwrap(), 0x1000);
    }

    #[test]
    fn parse_int_should_accept_h_suffix() {
        assert_eq!(parse_int("1FFFh", "test").unwrap(), 0x1fff);
    }

    #[test]
    fn parse_int_should_accept_negative() {
        assert_eq!(parse_int("-1", "test").unwrap(), -1);
    }

    #[test]
    fn parse_int_should_accept_underscores() {
        assert_eq!(parse_int("1_000", "test").unwrap(), 1000);
    }

    #[test]
    fn parse_int_should_error_on_invalid() {
        assert!(parse_int("not_a_number", "test").is_err());
    }

    #[test]
    fn human_size_should_return_zero_for_empty() {
        assert_eq!(human_size(0), "0 B");
    }

    #[test]
    fn human_size_should_format_bytes_below_1024() {
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn human_size_should_format_1_kib() {
        assert_eq!(human_size(1024), "1.00 KiB");
    }

    #[test]
    fn human_size_should_format_2_kib() {
        assert_eq!(human_size(2048), "2.00 KiB");
    }

    #[test]
    fn human_size_should_format_mib() {
        assert_eq!(human_size(1048576), "1.00 MiB");
    }

    #[test]
    fn scalar_json_should_parse_bool() {
        assert_eq!(scalar_json("true"), json!(true));
        assert_eq!(scalar_json("false"), json!(false));
    }

    #[test]
    fn scalar_json_should_parse_hex() {
        assert_eq!(scalar_json("0x10"), json!(16));
    }

    #[test]
    fn scalar_json_should_default_to_string() {
        assert_eq!(scalar_json("plain"), json!("plain"));
    }
}
