//! Shared parsing helpers for MTK scatter values.

use serde_json::Value;

use crate::scatter_parser::error::{Error, Result};

const NONE_TOKENS: &[&str] = &["", "NONE", "NULL", "N/A", "NA", "0"];

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

pub(crate) fn scalar_json(value: &str) -> Value {
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

pub(crate) fn value_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

pub(crate) fn parse_bool(value: Option<&Value>, default: bool) -> bool {
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

pub(crate) fn parse_field_int(
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

pub(crate) fn normalize_none_string(value: Option<&Value>) -> Option<String> {
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

pub(crate) fn get_first<'a>(map: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| map.get(*key))
}

pub(crate) fn find_general_value(general: &Value, wanted: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
