//! A dependency-free draft-07-subset JSON Schema validator.

use agent_core::{OutputSchema, Verdict};
use serde_json::Value;

/// Validates a value against the supported draft-07 subset (see the crate docs).
#[derive(Default)]
pub struct Draft07Validator;

impl Draft07Validator {
    pub fn new() -> Self {
        Self
    }
}

impl OutputSchema for Draft07Validator {
    fn validate(&self, schema: &Value, value: &Value) -> Verdict {
        let mut errors = Vec::new();
        check(schema, value, "", &mut errors);
        if errors.is_empty() {
            Verdict::pass()
        } else {
            Verdict::fail(errors)
        }
    }
}

/// Display form of a JSON path: the root is `(root)`, else `/a/b`.
fn disp(path: &str) -> &str {
    if path.is_empty() {
        "(root)"
    } else {
        path
    }
}

/// Recursively check `value` against `schema`, appending human-readable,
/// path-qualified errors. A non-object schema (e.g. `true`) matches anything.
fn check(schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return,
    };

    // `type` — a wrong type makes the remaining keyword checks meaningless.
    if let Some(t) = obj.get("type") {
        if !type_matches(t, value) {
            errors.push(format!(
                "{}: type mismatch (expected {t}, got {})",
                disp(path),
                type_name(value)
            ));
            return;
        }
    }

    // `enum` — membership by structural equality.
    if let Some(Value::Array(allowed)) = obj.get("enum") {
        if !allowed.iter().any(|a| a == value) {
            errors.push(format!("{}: value not in enum {allowed:?}", disp(path)));
        }
    }

    // Object keywords: required / properties / additionalProperties.
    if let Some(map) = value.as_object() {
        if let Some(Value::Array(req)) = obj.get("required") {
            for r in req.iter().filter_map(Value::as_str) {
                if !map.contains_key(r) {
                    errors.push(format!("{}/{r}: required property missing", disp(path)));
                }
            }
        }
        let props = obj.get("properties").and_then(Value::as_object);
        if let Some(props) = props {
            for (k, v) in map {
                if let Some(sub) = props.get(k) {
                    check(sub, v, &format!("{path}/{k}"), errors);
                }
            }
        }
        if obj.get("additionalProperties") == Some(&Value::Bool(false)) {
            for k in map.keys() {
                let known = props.map(|p| p.contains_key(k)).unwrap_or(false);
                if !known {
                    errors.push(format!("{path}/{k}: additional property not allowed"));
                }
            }
        }
    }

    // Array keywords: items / minItems / maxItems.
    if let Some(arr) = value.as_array() {
        if let Some(items) = obj.get("items") {
            for (i, elem) in arr.iter().enumerate() {
                check(items, elem, &format!("{path}/{i}"), errors);
            }
        }
        if let Some(min) = obj.get("minItems").and_then(Value::as_u64) {
            if (arr.len() as u64) < min {
                errors.push(format!("{}: fewer than {min} items", disp(path)));
            }
        }
        if let Some(max) = obj.get("maxItems").and_then(Value::as_u64) {
            if (arr.len() as u64) > max {
                errors.push(format!("{}: more than {max} items", disp(path)));
            }
        }
    }

    // Numeric bounds.
    if let Some(n) = value.as_f64() {
        if let Some(min) = obj.get("minimum").and_then(Value::as_f64) {
            if n < min {
                errors.push(format!("{}: below minimum {min}", disp(path)));
            }
        }
        if let Some(max) = obj.get("maximum").and_then(Value::as_f64) {
            if n > max {
                errors.push(format!("{}: above maximum {max}", disp(path)));
            }
        }
    }

    // String bounds (counted by char, not byte).
    if let Some(s) = value.as_str() {
        let len = s.chars().count() as u64;
        if let Some(min) = obj.get("minLength").and_then(Value::as_u64) {
            if len < min {
                errors.push(format!("{}: shorter than {min} chars", disp(path)));
            }
        }
        if let Some(max) = obj.get("maxLength").and_then(Value::as_u64) {
            if len > max {
                errors.push(format!("{}: longer than {max} chars", disp(path)));
            }
        }
    }
}

/// Whether `value` matches a `type` spec (a string or an array of strings).
fn type_matches(t: &Value, value: &Value) -> bool {
    match t {
        Value::String(s) => one_type(s, value),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .any(|s| one_type(s, value)),
        _ => true, // an unrecognised type spec doesn't fail
    }
}

fn one_type(t: &str, value: &Value) -> bool {
    match t {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        // A JSON integer, or a whole-valued float (12.0).
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false)
        }
        _ => true, // unknown type name → permissive
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Bench hook (dependency of `benches/validate.rs`): validate a value against a
/// schema and report the error count. Public but hidden.
#[doc(hidden)]
pub fn bench_validate(schema: &Value, value: &Value) -> usize {
    Draft07Validator::new().validate(schema, value).errors.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    // schema × value → Ok(()) on match, Err(substr) naming the failure.
    #[rstest]
    #[case::positive_type_match(
        json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
        json!({"n": 42}),
        Ok(()))]
    #[case::positive_enum_member(json!({"type": "string", "enum": ["a", "b", "c"]}), json!("b"), Ok(()))]
    #[case::positive_whole_float_is_integer(
        json!({"type": "integer"}), json!(12.0), Ok(()))]
    #[case::negative_required_missing(
        json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
        json!({}),
        Err("required"))]
    #[case::negative_type_mismatch(
        json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
        json!({"n": "not-a-number"}),
        Err("type"))]
    #[case::negative_additional_properties(
        json!({"type": "object", "properties": {"n": {"type": "integer"}}, "additionalProperties": false}),
        json!({"n": 1, "extra": true}),
        Err("additional"))]
    #[case::negative_enum_nonmember(json!({"type": "string", "enum": ["a", "b"]}), json!("z"), Err("enum"))]
    #[case::corner_nested_object_ok(
        json!({"type": "object", "properties": {
            "outer": {"type": "object", "properties": {"inner": {"type": "boolean"}}, "required": ["inner"]}},
            "required": ["outer"]}),
        json!({"outer": {"inner": true}}),
        Ok(()))]
    #[case::negative_nested_mismatch(
        json!({"type": "object", "properties": {
            "outer": {"type": "object", "properties": {"inner": {"type": "boolean"}}, "required": ["inner"]}},
            "required": ["outer"]}),
        json!({"outer": {"inner": 1}}),
        Err("inner"))]
    #[case::boundary_array_of_typed_items(
        json!({"type": "array", "items": {"type": "integer"}}), json!([1, 2, 3]), Ok(()))]
    #[case::negative_array_item_mismatch(
        json!({"type": "array", "items": {"type": "integer"}}), json!([1, "two"]), Err("/1"))]
    #[case::boundary_numeric_minimum(
        json!({"type": "number", "minimum": 0.0}), json!(-1), Err("minimum"))]
    #[case::boundary_string_maxlength(
        json!({"type": "string", "maxLength": 3}), json!("abcd"), Err("longer"))]
    fn validate_cases(
        #[case] schema: Value,
        #[case] value: Value,
        #[case] expected: std::result::Result<(), &str>,
    ) {
        let verdict = Draft07Validator::new().validate(&schema, &value);
        match expected {
            Ok(()) => assert!(verdict.ok, "expected pass, got {:?}", verdict.errors),
            Err(sub) => {
                assert!(!verdict.ok, "expected fail for {value}");
                assert!(
                    verdict.errors.iter().any(|e| e.contains(sub)),
                    "errors {:?} missing `{sub}`",
                    verdict.errors
                );
            }
        }
    }
}
