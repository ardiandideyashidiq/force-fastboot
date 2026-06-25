//! JSON serialization helpers for scatter partition data.

use std::path::Path;

use serde_json::{json, Value};

use crate::scatter_parser::parse::helpers::human_size;
use crate::scatter_parser::parse::image_magic;
use crate::scatter_parser::types::ScatterPartition;

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
    let resolved = crate::scatter_parser::path::resolve_image_path(
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
            if let Ok(meta) = std::fs::metadata(path) {
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
