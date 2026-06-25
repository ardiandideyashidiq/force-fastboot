//! Image file path resolution for scatter partitions.

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use tracing::warn;

use crate::scatter_parser::types::ResolvedPath;

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
        return empty_resolved_path();
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

    let candidates = build_candidates(original, &normalized, firmware_dir, scatter_dir);
    let meta = ResolveMeta { original, normalized: &normalized, absolute_input, input_style, contains_parent };

    let mut warning: Option<String> = None;
    if let Some(found) = check_existing_candidates(&candidates, package_root, &mut warning, &meta) {
        return found;
    }

    let first_allowed = find_first_allowed(&candidates, package_root);

    if image_search {
        let mut seen = std::collections::BTreeSet::new();
        for root in [firmware_dir, scatter_dir].into_iter().flatten() {
            let root = absolutize(root);
            if !seen.insert(root.clone()) {
                continue;
            }
            let basename = Path::new(&meta.normalized)
                .file_name()
                .unwrap_or_else(|| OsStr::new(&meta.normalized));
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
                        original: meta.original,
                        normalized: meta.normalized,
                        resolved_path: Some(found),
                        resolved_via: Some("image_search_unique_basename"),
                        exists: Some(true),
                        is_absolute_input: meta.absolute_input,
                        input_style: meta.input_style,
                        contains_parent_reference: meta.contains_parent,
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
            original: meta.original,
            normalized: meta.normalized,
            resolved_path: Some(candidate),
            resolved_via: Some(via),
            exists: Some(false),
            is_absolute_input: meta.absolute_input,
            input_style: meta.input_style,
            contains_parent_reference: meta.contains_parent,
            outside_package_root: outside,
            warning,
        });
    }
    resolved_path_result(ResolvedPathParts {
        original: meta.original,
        normalized: meta.normalized,
        resolved_path: None,
        resolved_via: None,
        exists: Some(false),
        is_absolute_input: meta.absolute_input,
        input_style: meta.input_style,
        contains_parent_reference: meta.contains_parent,
        outside_package_root: package_root.as_ref().map(|_| true),
        warning: warning.or_else(|| Some("no allowed image path candidate".to_string())),
    })
}

const fn empty_resolved_path() -> ResolvedPath {
    ResolvedPath {
        original: None, normalized: None, resolved_path: None,
        resolved_via: None, exists: None, is_absolute_input: false,
        input_style: None, contains_parent_reference: false,
        outside_package_root: None, warning: None,
    }
}

struct ResolveMeta<'a> {
    original: &'a str,
    normalized: &'a str,
    absolute_input: bool,
    input_style: &'a str,
    contains_parent: bool,
}

fn build_candidates<'a>(
    original: &'a str,
    normalized: &'a str,
    firmware_dir: Option<&Path>,
    scatter_dir: Option<&Path>,
) -> Vec<(&'a str, PathBuf)> {
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
    if normalized.starts_with('/') {
        candidates.push(("absolute", PathBuf::from(normalized)));
    } else if is_windows_absolute(original) {
        candidates.push(("windows_absolute", PathBuf::from(original)));
        let stripped = mixed_parts_path(original);
        if let Some(fd) = firmware_dir {
            candidates.push(("firmware_dir_windows_stripped", fd.join(&stripped)));
        }
        if let Some(sd) = scatter_dir {
            candidates.push(("scatter_relative_windows_stripped", sd.join(&stripped)));
        }
    } else {
        let rel = mixed_parts_path(normalized);
        if let Some(fd) = firmware_dir {
            candidates.push(("firmware_dir_relative", fd.join(&rel)));
        }
        if let Some(sd) = scatter_dir {
            candidates.push(("scatter_relative", sd.join(&rel)));
        }
    }
    candidates
}

fn check_existing_candidates(
    candidates: &[(&str, PathBuf)],
    package_root: Option<&Path>,
    warning: &mut Option<String>,
    meta: &ResolveMeta<'_>,
) -> Option<ResolvedPath> {
    for &(via, ref candidate) in candidates {
        let candidate = absolutize(candidate);
        let outside = package_root.as_ref().map(|root| !is_within(&candidate, root));
        if outside == Some(true) {
            *warning = Some(format!("resolved image path is outside package_root: {}", candidate.display()));
            continue;
        }
        if candidate.exists() {
            return Some(resolved_path_result(ResolvedPathParts {
                original: meta.original,
                normalized: meta.normalized,
                resolved_path: Some(candidate),
                resolved_via: Some(via),
                exists: Some(true),
                is_absolute_input: meta.absolute_input,
                input_style: meta.input_style,
                contains_parent_reference: meta.contains_parent,
                outside_package_root: outside,
                warning: warning.clone(),
            }));
        }
    }
    None
}

fn find_first_allowed<'a>(
    candidates: &'a [(&'a str, PathBuf)],
    package_root: Option<&Path>,
) -> Option<(&'a str, PathBuf, Option<bool>)> {
    candidates.iter().find_map(|&(via, ref candidate)| {
        let candidate = absolutize(candidate);
        let outside = package_root.as_ref().map(|root| !is_within(&candidate, root));
        if outside == Some(true) {
            None
        } else {
            Some((via, candidate, outside))
        }
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
                if !out.pop() {
                    // Retain `..` above root for relative paths instead of
                    // silently dropping it.
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_outside_package_is_blocked() {
        let result = resolve_image_path(
            Some("../outside.img"),
            Some(Path::new("/tmp/scatter_dir")),
            None,
            Some(Path::new("/tmp/scatter_dir")),
            false,
        );
        assert_eq!(result.exists, Some(false), "outside path should not exist");
        assert_eq!(result.outside_package_root, Some(true));
    }

    #[test]
    fn resolve_inside_package_works() {
        let result = resolve_image_path(
            Some("nonexistent.img"),
            Some(Path::new("/tmp")),
            None,
            Some(Path::new("/tmp")),
            false,
        );
        assert!(result.normalized.is_some(), "normalized should be set");
    }
}
