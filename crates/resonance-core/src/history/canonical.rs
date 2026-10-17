//! Canonical JSON (FR-153, MVP-026): object keys sorted by code point, no insignificant
//! whitespace, numbers in their shortest round-trip form, and strings escaped only where JSON
//! requires it. The same value therefore always has the same bytes.

use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

/// Serialize any value canonically.
pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
    let v = serde_json::to_value(value)?;
    let mut out = String::new();
    write(&v, &mut out);
    Ok(out)
}

/// Append the canonical form of `v` to `out`.
pub fn write(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                write(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        // serde_json writes scalars deterministically (shortest round-trip numbers).
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_at_every_depth() {
        let a = json!({"b": 1, "a": {"z": [1, {"y": 2, "x": 0.25}], "c": "q\"u"}});
        assert_eq!(
            to_string(&a).unwrap(),
            r#"{"a":{"c":"q\"u","z":[1,{"x":0.25,"y":2}]},"b":1}"#
        );
    }

    #[test]
    fn equal_values_have_equal_bytes() {
        let a: Value = serde_json::from_str(r#"{"x":1.0e-1,"y":[true,null]}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{ "y" : [ true , null ], "x" : 0.1 }"#).unwrap();
        assert_eq!(to_string(&a).unwrap(), to_string(&b).unwrap());
    }
}
