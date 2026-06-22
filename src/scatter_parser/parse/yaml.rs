use std::collections::BTreeMap;
use serde_json::{json, Map, Value};
use super::{ParsedRawScatter, scalar_json, value_to_string, find_general_value};

pub(crate) fn parse_yaml_scatter(text: &str) -> ParsedRawScatter {
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

    // Intent: plumb warnings through ParsedRawScatter when callers need them.
    drop(warnings);

    ParsedRawScatter {
        general: general_value,
        layouts,
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
