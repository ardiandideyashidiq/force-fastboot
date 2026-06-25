use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::scatter_parser::parse::human_size;
use crate::scatter_parser::types::{FlashAction, ScatterPartition};
use crate::scatter_parser::util::split_base_slot;

use super::image::recheck_synthesized_image;

pub(super) fn inherited_image_source_for_slot_b<'a>(
    part: &'a ScatterPartition,
    parts_by_name: &BTreeMap<String, &'a ScatterPartition>,
) -> &'a ScatterPartition {
    if part.slot().as_deref() != Some("b") || part.flashable_by_profile() {
        return part;
    }

    let source_name = format!("{}_a", part.base_name());
    match parts_by_name.get(&source_name) {
        Some(source) if source.flashable_by_profile() => source,
        _ => part,
    }
}

pub(super) fn inherited_action_reason(
    base_reason: String,
    part: &ScatterPartition,
    image_source: &ScatterPartition,
) -> String {
    if part.name.eq_ignore_ascii_case(&image_source.name) {
        return base_reason;
    }
    let Some(source_slot) = image_source.slot() else {
        return base_reason;
    };
    format!("{base_reason}; inherited from slot {source_slot} image")
}

pub(super) fn expand_requested_names(
    requested: &[String],
    available: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in requested {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let lname = name.to_lowercase();
        if available.contains(&lname) || split_base_slot(&lname).1.is_some() {
            out.insert(lname);
            continue;
        }
        let mut added = false;
        for slot in ["a", "b"] {
            let candidate = format!("{lname}_{slot}");
            if available.contains(&candidate) {
                out.insert(candidate);
                added = true;
            }
        }
        if !added {
            out.insert(lname);
        }
    }
    out
}

pub(super) fn synthesize_slot_actions_if_needed(
    selected_parts: &[ScatterPartition],
    actions: &mut Vec<FlashAction>,
) {
    synthesize_non_download_slot_actions(selected_parts, actions);
}

fn synthesize_non_download_slot_actions(
    selected_parts: &[ScatterPartition],
    actions: &mut Vec<FlashAction>,
) {
    let parts_by_name: BTreeMap<_, _> = selected_parts
        .iter()
        .map(|p| (p.name.to_lowercase(), p))
        .collect();
    let actions_by_partition: BTreeMap<_, _> = actions
        .iter()
        .filter(|a| a.action == "flash")
        .map(|a| (a.partition.to_lowercase(), a.clone()))
        .collect();
    let planned: BTreeSet<_> = actions_by_partition.keys().cloned().collect();
    let mut synthesized = Vec::new();

    for source_action in actions_by_partition.values() {
        let Some(source_slot) = source_action.slot.as_deref() else {
            continue;
        };
        let target_slot = match source_slot {
            "a" => "b",
            "b" => "a",
            _ => continue,
        };
        let target_name = format!("{}_{}", source_action.base_name, target_slot);
        if planned.contains(&target_name) {
            continue;
        }
        let Some(target_part) = parts_by_name.get(&target_name) else {
            continue;
        };
        if target_part.flashable_by_profile() {
            continue;
        }
        synthesized.push(slot_synthesized_action(
            source_action,
            target_part,
            source_slot,
        ));
    }

    actions.extend(synthesized);
}

fn slot_synthesized_action(
    source: &FlashAction,
    target: &ScatterPartition,
    source_slot: &str,
) -> FlashAction {
    let (image, warnings) = recheck_synthesized_image(source.image.clone(), target);
    FlashAction {
        action: source.action.clone(),
        partition: target.name.clone(),
        base_name: target.base_name(),
        slot: target.slot(),
        layout: target.layout.clone(),
        region: target.region.clone(),
        start: target.linear_start,
        start_hex: format!("{:#x}", target.linear_start),
        size: target.size,
        size_hex: format!("{:#x}", target.size),
        size_human: human_size(target.size),
        image,
        image_type: target.image_type.clone(),
        safety_class: target.safety_class(),
        reason: format!("inferred from slot {source_slot} image for slot all"),
        warnings,
    }
}

pub(super) fn check_incomplete_slots(
    selected_parts: &[ScatterPartition],
    actions: &[FlashAction],
    allow_incomplete_slots: bool,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> BTreeMap<String, Value> {
    let mut incomplete_slots = BTreeMap::new();

    let mut by_base_available: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut by_base_planned: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for part in selected_parts {
        if let Some(slot) = part.slot() {
            by_base_available
                .entry(part.base_name())
                .or_default()
                .insert(slot);
        }
    }
    for action in actions.iter().filter(|a| a.action == "flash") {
        if let Some(slot) = &action.slot {
            by_base_planned
                .entry(action.base_name.clone())
                .or_default()
                .insert(slot.clone());
        }
    }

    for (base, available) in by_base_available {
        if !available.is_superset(&BTreeSet::from(["a".to_string(), "b".to_string()])) {
            continue;
        }
        let planned = by_base_planned.get(&base).cloned().unwrap_or_default();
        if !planned.is_empty()
            && planned != BTreeSet::from(["a".to_string(), "b".to_string()])
        {
            let available_slots: Vec<_> = available.iter().cloned().collect();
            let planned_slots: Vec<_> = planned.iter().cloned().collect();
            incomplete_slots.insert(
                base.clone(),
                json!({
                    "available_slots": available_slots,
                    "planned_slots": planned_slots,
                }),
            );
            let msg = format!(
                "slot policy both requested but only planned slots {planned_slots:?} for {base}; available slots are {available_slots:?}"
            );
            if allow_incomplete_slots {
                warnings.push(msg);
            } else {
                errors.push(msg);
            }
        }
    }
    incomplete_slots
}
