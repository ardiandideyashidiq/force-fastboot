use std::path::Path;
use serde_json::{json, Value};
use crate::scatter_parser::parse::{human_size, image_magic};
use crate::scatter_parser::path::resolve_image_path;
use crate::scatter_parser::types::{FlashPlanOptions, ScatterPartition};

pub(crate) fn resolve_images_for_plan(
    part: &ScatterPartition,
    scatter_dir: Option<&Path>,
    options: &FlashPlanOptions,
) -> (Value, Vec<String>) {
    let resolved = resolve_image_path(
        part.file_name.as_deref(),
        scatter_dir,
        options.firmware_dir.as_deref(),
        options.package_root.as_deref(),
        options.image_search,
    );
    let (status, mut warnings) = checked_image_status(
        resolved.resolved_path.as_deref(),
        resolved.exists,
        options.check_images,
        part.size,
    );
    if let Some(warning) = &resolved.warning {
        warnings.insert(0, warning.clone());
    }
    (
        json!({
            "file_name": part.file_name,
            "path": resolved,
            "status": status,
        }),
        warnings,
    )
}

pub(crate) fn checked_image_status(
    resolved_path: Option<&str>,
    exists: Option<bool>,
    checked: bool,
    target_size: i64,
) -> (Value, Vec<String>) {
    let mut warnings = Vec::new();
    let mut status = json!({
        "checked": checked,
        "exists": exists,
        "size_bytes": null,
        "size_human": null,
        "fits_partition": null,
        "magic": null,
    });
    if !checked {
        return (status, warnings);
    }

    if let Some(path) = resolved_path.filter(|_| exists == Some(true)) {
        match std::fs::metadata(path) {
            Ok(meta) => {
                if let Ok(size) = i64::try_from(meta.len()) {
                    status["size_bytes"] = json!(size);
                    status["size_human"] = json!(human_size(size));
                    status["fits_partition"] = json!(size <= target_size);
                    status["magic"] = json!(image_magic(Path::new(path)));
                    if size > target_size {
                        warnings.push(format!(
                            "image is larger than partition: {size} > {target_size}"
                        ));
                    }
                }
            }
            Err(e) => warnings.push(format!("failed to stat image: {e}")),
        }
    } else {
        warnings.push("image missing".to_string());
    }
    (status, warnings)
}

pub(crate) fn recheck_synthesized_image(
    image: Option<Value>,
    target: &ScatterPartition,
) -> (Option<Value>, Vec<String>) {
    let Some(mut image) = image else {
        return (None, Vec::new());
    };
    let mut warnings = Vec::new();
    if let Some(warning) = image.pointer("/path/warning").and_then(Value::as_str) {
        warnings.push(warning.to_string());
    }
    let checked = image
        .pointer("/status/checked")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !checked {
        return (Some(image), warnings);
    }

    let (status, mut status_warnings) = checked_image_status(
        image.pointer("/path/resolved_path").and_then(Value::as_str),
        image.pointer("/path/exists").and_then(Value::as_bool),
        true,
        target.size,
    );
    warnings.append(&mut status_warnings);
    image["status"] = status;
    (Some(image), warnings)
}
