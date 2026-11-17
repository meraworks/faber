//! Reads tool input. A type error names its JSON path.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::borrow::Cow;

/// Keys that name the variant of an internally tagged enum.
const TAGS: [&str; 3] = ["tool", "op", "kind"];

/// One step of a JSON path.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
}

/// Reads a typed value. The error names the path of the bad value.
/// The prefix is the path of the value, such as `operations[1]`. An empty prefix gives paths from the root.
pub fn read<T: DeserializeOwned>(value: &Value, prefix: &str) -> Result<T, String> {
    let error = match serde_json::from_value::<T>(value.clone()) {
        Ok(typed) => return Ok(typed),
        Err(error) => error.to_string(),
    };
    // A missing or unknown field has a position. Other errors need trials.
    let steps = if error.starts_with("missing field") || error.starts_with("unknown field") {
        by_position::<T>(value)
    } else {
        by_trial::<T>(value, &error)
    };
    Err(with_path(&path_text(prefix, &steps), &error))
}

/// Wraps a tool argument type. Its errors name the path, as `read` does.
pub struct Checked<T>(pub T);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Checked<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        read(&value, "").map(Checked).map_err(D::Error::custom)
    }
}

impl<T: JsonSchema> JsonSchema for Checked<T> {
    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}

/// Finds the object that holds a missing or unknown field, from the error position.
/// The text has one line, so the error column is the byte offset plus one.
fn by_position<T: DeserializeOwned>(root: &Value) -> Vec<Step> {
    let mut text = String::new();
    let mut spans = Vec::new();
    render(root, &mut Vec::new(), &mut text, &mut spans);
    match serde_json::from_str::<T>(&text) {
        Err(error) if error.line() == 1 => {
            let offset = error.column().saturating_sub(1);
            spans
                .iter()
                .filter(|span| span.start <= offset && offset < span.end)
                .min_by_key(|span| span.end - span.start)
                .map(|span| span.steps.clone())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// The byte range and path of one value in the rendered text.
struct Span {
    start: usize,
    end: usize,
    steps: Vec<Step>,
}

/// Writes a value as one line of JSON. Records the range of each value.
fn render(value: &Value, steps: &mut Vec<Step>, out: &mut String, spans: &mut Vec<Span>) {
    let start = out.len();
    match value {
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                steps.push(Step::Index(index));
                render(item, steps, out, spans);
                steps.pop();
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                steps.push(Step::Key(key.clone()));
                render(item, steps, out, spans);
                steps.pop();
            }
            out.push('}');
        }
        leaf => out.push_str(&leaf.to_string()),
    }
    spans.push(Span {
        start,
        end: out.len(),
        steps: steps.clone(),
    });
}

/// Walks down from the root to the value that holds the error.
/// A trial removes a key, or cuts a list. The error lies in the child when the message changes.
fn by_trial<T: DeserializeOwned>(root: &Value, message: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    while let Some(step) = next_step::<T>(root, &steps, message) {
        steps.push(step);
    }
    steps
}

fn next_step<T: DeserializeOwned>(root: &Value, steps: &[Step], message: &str) -> Option<Step> {
    match node_at(root, steps)? {
        Value::Object(map) => map
            .keys()
            .filter(|key| !TAGS.contains(&key.as_str()))
            .find(|key| {
                let mut trial = root.clone();
                if let Some(Value::Object(parent)) = node_mut(&mut trial, steps) {
                    parent.remove(key.as_str());
                }
                trial_error::<T>(&trial).as_deref() != Some(message)
            })
            .map(|key| Step::Key(key.clone())),
        Value::Array(items) => {
            let cut = |length: usize| {
                let mut trial = root.clone();
                if let Some(Value::Array(list)) = node_mut(&mut trial, steps) {
                    list.truncate(length);
                }
                trial_error::<T>(&trial)
            };
            // A list that still fails with no items holds the error itself.
            if cut(0).as_deref() == Some(message) {
                return None;
            }
            for length in 0..=items.len() {
                let error = cut(length);
                // Fixed size lists fail with a length error when cut. Their items are not blamed.
                if length < items.len()
                    && error
                        .as_deref()
                        .is_some_and(|e| e.contains("invalid length"))
                {
                    return None;
                }
                if length > 0 && error.as_deref() == Some(message) {
                    return Some(Step::Index(length - 1));
                }
            }
            None
        }
        _ => None,
    }
}

fn trial_error<T: DeserializeOwned>(value: &Value) -> Option<String> {
    serde_json::from_value::<T>(value.clone())
        .err()
        .map(|error| error.to_string())
}

fn node_at<'a>(root: &'a Value, steps: &[Step]) -> Option<&'a Value> {
    steps.iter().try_fold(root, |node, step| match step {
        Step::Key(key) => node.get(key.as_str()),
        Step::Index(index) => node.get(*index),
    })
}

fn node_mut<'a>(root: &'a mut Value, steps: &[Step]) -> Option<&'a mut Value> {
    steps.iter().try_fold(root, |node, step| match step {
        Step::Key(key) => node.get_mut(key.as_str()),
        Step::Index(index) => node.get_mut(*index),
    })
}

/// Writes the path, such as `operations[1].feature.geometry.faces`.
fn path_text(prefix: &str, steps: &[Step]) -> String {
    let mut text = prefix.to_string();
    for step in steps {
        match step {
            Step::Index(index) => text.push_str(&format!("[{index}]")),
            Step::Key(key) if is_plain(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key);
            }
            Step::Key(key) => text.push_str(&format!("[{}]", Value::String(key.clone()))),
        }
    }
    text
}

/// A plain key is a name such as `faces`. Other keys go in brackets.
fn is_plain(key: &str) -> bool {
    key.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn with_path(path: &str, message: &str) -> String {
    if path.is_empty() {
        message.to_string()
    } else {
        format!("{path}: {message}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Inner {
        name: String,
    }

    #[derive(Debug, Deserialize)]
    struct Outer {
        items: Vec<Inner>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Keyed {
        parts: std::collections::BTreeMap<String, Inner>,
    }

    /// An internally tagged enum, as the model uses for features.
    #[derive(Debug, Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
    #[allow(dead_code)]
    enum Shape {
        Shell {
            source: String,
            faces: String,
            thickness: f64,
        },
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Feature {
        id: String,
        geometry: Shape,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Edit {
        tool: String,
        feature: Feature,
    }

    #[test]
    fn valid_value_reads_without_a_path() {
        let value = json!({"items":[{"name":"a"}]});
        let outer: Outer = read(&value, "").unwrap();
        assert_eq!(outer.items[0].name, "a");
    }

    #[test]
    fn wrong_type_in_a_list_item_names_the_full_path() {
        let value = json!({"items":[{"name":"a"},{"name":["x"]}]});
        let error = read::<Outer>(&value, "").unwrap_err();
        assert!(error.starts_with("items[1].name: "), "{error}");
        assert!(error.contains("invalid type"), "{error}");
    }

    #[test]
    fn prefix_joins_the_path_of_the_value() {
        let value = json!({"name":{"deep":1}});
        let error = read::<Inner>(&value, "operations[1]").unwrap_err();
        assert!(error.starts_with("operations[1].name: "), "{error}");
    }

    #[test]
    fn unknown_field_names_the_object_that_holds_it() {
        let value = json!({"items":[{"name":"a","colr":1}]});
        let error = read::<Outer>(&value, "").unwrap_err();
        assert!(error.starts_with("items[0]: "), "{error}");
        assert!(error.contains("unknown field `colr`"), "{error}");
    }

    #[test]
    fn missing_field_at_the_root_has_no_path() {
        let error = read::<Inner>(&json!({}), "").unwrap_err();
        assert_eq!(error, "missing field `name`");
    }

    #[test]
    fn missing_field_names_the_object_that_lacks_it() {
        let value = json!({"items":[{"name":"a"},{}]});
        let error = read::<Outer>(&value, "").unwrap_err();
        assert_eq!(error, "items[1]: missing field `name`");
    }

    #[test]
    fn map_key_with_a_dash_is_bracketed() {
        let value = json!({"parts":{"a-b":{"name":3}}});
        let error = read::<Keyed>(&value, "").unwrap_err();
        assert!(error.starts_with("parts[\"a-b\"].name: "), "{error}");
    }

    #[test]
    fn non_ascii_text_before_the_error_keeps_the_path_exact() {
        let value = json!({"items":[{"name":"Grüße Ω"},{"name":5}]});
        let error = read::<Outer>(&value, "").unwrap_err();
        assert!(error.starts_with("items[1].name: "), "{error}");
    }

    #[test]
    fn checked_wrapper_names_the_path_in_its_error() {
        let value = json!({"items":[{"name":[1]}]});
        let error = serde_json::from_value::<Checked<Outer>>(value)
            .err()
            .unwrap();
        assert!(error.to_string().contains("items[0].name: "), "{error}");
    }

    #[test]
    fn error_inside_a_tagged_enum_names_the_field() {
        // The tagged enum buffers its fields, so the position alone is not enough.
        let value = json!({"tool":"add","feature":{"id":"walls","geometry":{
            "op":"shell","source":"block","faces":[[0,0,1]],"thickness":1}}});
        let error = read::<Edit>(&value, "operations[1]").unwrap_err();
        assert!(
            error.starts_with("operations[1].feature.geometry.faces: "),
            "{error}"
        );
    }

    #[test]
    fn missing_field_inside_a_tagged_enum_names_the_object() {
        let value = json!({"tool":"add","feature":{"id":"walls","geometry":{
            "op":"shell","source":"block","thickness":1}}});
        let error = read::<Edit>(&value, "").unwrap_err();
        assert!(error.contains("missing field `faces`"), "{error}");
        assert!(error.starts_with("feature"), "{error}");
    }

    #[test]
    fn wrong_item_in_a_list_names_the_index() {
        let value = json!({"items":[{"name":"a"},{"name":"b"},{"name":{"x":1}}]});
        let error = read::<Outer>(&value, "").unwrap_err();
        assert!(error.starts_with("items[2].name: "), "{error}");
    }
}
