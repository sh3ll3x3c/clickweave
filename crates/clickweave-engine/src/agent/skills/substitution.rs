//! Template substitution over JSON values for `{{params.X}}` and
//! `{{captured.X}}` placeholders. Supports nested-path resolution
//! (`{{captured.x.y[0]}}`); whole-string templates resolve to their
//! referenced JSON value, preserving the source type.

#![allow(dead_code)]

use std::collections::HashMap;

use serde_json::Value;

use super::types::SkillError;

const PARAMS_PREFIX: &str = "{{params.";
const CAPTURED_PREFIX: &str = "{{captured.";
const TEMPLATE_SUFFIX: &str = "}}";

pub fn substitute_value(
    value: &Value,
    params: &Value,
    captured: &HashMap<String, Value>,
) -> Result<Value, SkillError> {
    match value {
        Value::String(s) => substitute_string(s, params, captured),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(substitute_value(item, params, captured)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), substitute_value(v, params, captured)?);
            }
            Ok(Value::Object(out))
        }
        _ => Ok(value.clone()),
    }
}

fn substitute_string(
    s: &str,
    params: &Value,
    captured: &HashMap<String, Value>,
) -> Result<Value, SkillError> {
    if let Some(path) = strip_template(s, PARAMS_PREFIX) {
        return resolve_jsonpath(params, path).ok_or_else(|| {
            SkillError::Substitution(format!("undefined params reference: {path}"))
        });
    }
    if let Some(path) = strip_template(s, CAPTURED_PREFIX) {
        let head = path.split(['.', '[']).next().unwrap_or("");
        let root = captured.get(head).ok_or_else(|| {
            SkillError::Substitution(format!("undefined captured reference: {head}"))
        })?;
        let rest = path
            .strip_prefix(head)
            .unwrap_or("")
            .trim_start_matches('.');
        if rest.is_empty() {
            return Ok(root.clone());
        }
        return resolve_jsonpath(root, rest)
            .ok_or_else(|| SkillError::Substitution(format!("undefined captured path: {path}")));
    }
    Ok(Value::String(s.to_string()))
}

fn strip_template<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.starts_with(prefix) && s.ends_with(TEMPLATE_SUFFIX) {
        Some(&s[prefix.len()..s.len() - TEMPLATE_SUFFIX.len()])
    } else {
        None
    }
}

fn resolve_jsonpath(root: &Value, path: &str) -> Option<Value> {
    let mut current = root;
    let mut segment = String::new();
    let mut in_index = false;
    for ch in path.chars() {
        match ch {
            '.' if !in_index => {
                if !segment.is_empty() {
                    current = current.get(&segment)?;
                    segment.clear();
                }
            }
            '[' => {
                if !segment.is_empty() {
                    current = current.get(&segment)?;
                    segment.clear();
                }
                in_index = true;
            }
            ']' => {
                let idx: usize = segment.parse().ok()?;
                current = current.get(idx)?;
                segment.clear();
                in_index = false;
            }
            c => segment.push(c),
        }
    }
    if !segment.is_empty() {
        current = current.get(&segment)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn captured_map(entries: &[(&str, Value)]) -> HashMap<String, Value> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn resolves_params_reference() {
        let v = json!("{{params.contact_name}}");
        let r = substitute_value(&v, &json!({"contact_name":"Vesna"}), &HashMap::new()).unwrap();
        assert_eq!(r, json!("Vesna"));
    }

    #[test]
    fn resolves_captured_reference() {
        let v = json!("{{captured.uid}}");
        let r =
            substitute_value(&v, &json!({}), &captured_map(&[("uid", json!("a42g3"))])).unwrap();
        assert_eq!(r, json!("a42g3"));
    }

    #[test]
    fn nested_path_resolves() {
        let v = json!("{{captured.row.uid}}");
        let r = substitute_value(
            &v,
            &json!({}),
            &captured_map(&[("row", json!({"uid":"a42g3"}))]),
        )
        .unwrap();
        assert_eq!(r, json!("a42g3"));
    }

    #[test]
    fn array_index_resolves() {
        let v = json!("{{captured.list[0]}}");
        let r = substitute_value(
            &v,
            &json!({}),
            &captured_map(&[("list", json!(["first", "second"]))]),
        )
        .unwrap();
        assert_eq!(r, json!("first"));
    }

    #[test]
    fn undefined_reference_errors() {
        let v = json!("{{params.missing}}");
        let r = substitute_value(&v, &json!({}), &HashMap::new());
        assert!(matches!(r, Err(SkillError::Substitution(_))));
    }

    #[test]
    fn substitution_recurses_into_nested_object() {
        let v = json!({ "a": { "b": "{{params.x}}" }, "c": ["{{params.y}}"] });
        let r = substitute_value(&v, &json!({"x":"X","y":"Y"}), &HashMap::new()).unwrap();
        assert_eq!(r, json!({ "a": { "b": "X" }, "c": ["Y"] }));
    }

    #[test]
    fn non_template_string_passes_through_unchanged() {
        let v = json!("hello world");
        let r = substitute_value(&v, &json!({}), &HashMap::new()).unwrap();
        assert_eq!(r, json!("hello world"));
    }
}
