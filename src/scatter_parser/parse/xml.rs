use std::collections::BTreeMap;
use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};

use crate::scatter_parser::error::{Error, Result};
use super::{ParsedRawScatter, scalar_json, value_to_string, find_general_value};

#[derive(Debug, Clone, Default)]
pub(crate) struct XmlNode {
    pub(crate) tag: String,
    pub(crate) attrs: Map<String, Value>,
    pub(crate) text: String,
    pub(crate) children: Vec<XmlNode>,
}

impl XmlNode {
    pub(crate) fn descendants(&self) -> Vec<&XmlNode> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.descendants());
        }
        out
    }
}

pub(crate) fn parse_xml_scatter(text: &str) -> Result<ParsedRawScatter> {
    let root = parse_xml_node(text).map_err(Error::Xml)?;

    if is_checksum_scatter(&root) {
        return Ok(ParsedRawScatter {
            general: json!({}),
            layouts: BTreeMap::new(),
            platform: None,
            project: None,
            format: "checksum_xml".to_string(),
        });
    }

    let general = parse_general_info(&root);
    let layouts = parse_layouts(&root);

    let general_value = Value::Object(general);
    let platform = find_general_value(&general_value, "platform");
    let project = find_general_value(&general_value, "project");
    Ok(ParsedRawScatter {
        general: general_value,
        layouts,
        platform,
        project,
        format: "xml".to_string(),
    })
}

fn is_checksum_scatter(root: &XmlNode) -> bool {
    matches!(
        root.tag.to_lowercase().as_str(),
        "scatter" | "checksum" | "scatter_checksum"
    ) && !root
        .descendants()
        .iter()
        .any(|node| node.tag == "partition_index")
}

fn parse_general_info(root: &XmlNode) -> Map<String, Value> {
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
    general
}

fn parse_layouts(root: &XmlNode) -> BTreeMap<String, Vec<Map<String, Value>>> {
    let mut layouts: BTreeMap<String, Vec<Map<String, Value>>> = BTreeMap::new();

    for storage_node in root
        .descendants()
        .into_iter()
        .filter(|node| node.tag == "storage_type")
    {
        let layout = layout_name(storage_node);
        for part_node in storage_node
            .descendants()
            .into_iter()
            .filter(|node| node.tag == "partition_index")
        {
            let mut entry = xml_children_dict(part_node);
            let index = partition_index_attr(part_node, &entry);
            if let Some(index) = index {
                entry.insert("partition_index".to_string(), Value::String(index));
            }
            layouts.entry(layout.clone()).or_default().push(entry);
        }
    }

    if layouts.is_empty() {
        let direct_parts = collect_direct_partitions(root);
        if !direct_parts.is_empty() {
            let layout_name = infer_layout_name(&direct_parts);
            layouts.insert(layout_name, direct_parts);
        }
    }

    if layouts.is_empty() {
        let all_parts = collect_all_partitions(root);
        if !all_parts.is_empty() {
            layouts.insert("DEFAULT".to_string(), all_parts);
        }
    }

    layouts
}

fn layout_name(storage_node: &XmlNode) -> String {
    value_to_string(
        storage_node
            .attrs
            .get("name")
            .or_else(|| storage_node.attrs.get("value")),
    )
    .or_else(|| {
        let text = storage_node.text.trim();
        (!text.is_empty()).then(|| text.to_string())
    })
    .unwrap_or_else(|| "UNKNOWN".to_string())
}

fn partition_index_attr(part_node: &XmlNode, entry: &Map<String, Value>) -> Option<String> {
    value_to_string(
        part_node
            .attrs
            .get("name")
            .or_else(|| part_node.attrs.get("value")),
    )
    .or_else(|| value_to_string(entry.get("partition_index")))
}

fn collect_direct_partitions(root: &XmlNode) -> Vec<Map<String, Value>> {
    root
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
        .collect()
}

fn infer_layout_name(parts: &[Map<String, Value>]) -> String {
    let mut joined = String::new();
    for entry in parts {
        let _ = std::fmt::write(&mut joined, format_args!(
            "{} {}",
            value_to_string(entry.get("storage")).unwrap_or_default(),
            value_to_string(entry.get("region")).unwrap_or_default()
        ));
    }
    let joined = joined.to_uppercase();
    if joined.contains("UFS") { "UFS" } else { "EMMC" }.to_string()
}

fn collect_all_partitions(root: &XmlNode) -> Vec<Map<String, Value>> {
    root
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
        .collect()
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
        if buf.capacity() > 1_048_576 {
            buf.shrink_to(4096);
        }
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

fn strip_ns(tag: &str) -> String {
    tag.rsplit('}').next().unwrap_or(tag).to_string()
}
