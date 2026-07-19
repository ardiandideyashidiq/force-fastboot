use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::scatter_parser::error::{Error, Result};
use crate::scatter_parser::parse::helpers::{
    get_first, normalize_none_string, parse_bool, parse_field_int, value_to_string,
};
use crate::scatter_parser::types::ScatterPartition;
use crate::scatter_parser::util::{region_family, storage_family};

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

pub(super) fn normalize_partition(
    path: &Path,
    layout: &str,
    entry: Map<String, Value>,
) -> Result<ScatterPartition> {
    let name = normalize_none_string(get_first(&entry, &["partition_name", "name"]))
        .ok_or_else(|| Error::InvalidValue {
            detail: format!(
                "partition without partition_name in layout {layout}: {entry:?}"
            ),
            source_text: None,
            span: None,
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

pub(super) fn validate_layouts(
    layouts: &BTreeMap<String, Vec<ScatterPartition>>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    for (layout, parts) in layouts {
        let mut seen: HashMap<
            (String, String),
            &ScatterPartition,
        > = HashMap::new();
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
