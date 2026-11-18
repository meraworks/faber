//! Reads recipe shapes for the schema tool. Each shape comes from the schemars schema of its model type.

use faber_model::{
    EdgeSelector, Edit, FaceSelector, Feature, Geometry, Part, Scalar, SectionAnalysis, SketchLoop,
};
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// The recipe kinds that the schema tool reads.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Part,
    Feature,
    Geometry,
    SketchLoop,
    Selector,
    Section,
    Edit,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Part => "part",
            Kind::Feature => "feature",
            Kind::Geometry => "geometry",
            Kind::SketchLoop => "sketch_loop",
            Kind::Selector => "selector",
            Kind::Section => "section",
            Kind::Edit => "edit",
        }
    }
}

/// Arguments of the schema tool.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaArgs {
    /// The recipe kind to read.
    kind: Kind,
    /// An op name for kind geometry. Omit it to read the op list.
    op: Option<String>,
}

/// Reads the shape of one kind. Kind geometry gives one line for each op.
pub fn read(args: SchemaArgs) -> Result<Value, String> {
    match (args.kind, args.op) {
        (Kind::Geometry, None) => {
            let root = schema_json::<Geometry<Scalar>>();
            Ok(json!({"schema":1,"kind":"geometry","ops":op_lines(&root)}))
        }
        (Kind::Geometry, Some(op)) => op_schema(&schema_json::<Geometry<Scalar>>(), &op),
        (kind, None) => Ok(json!({"schema":1,"kind":kind.name(),"definition":definition(kind)})),
        (kind, Some(_)) => Err(format!("Kind {} has no op. Omit op.", kind.name())),
    }
}

fn definition(kind: Kind) -> Value {
    match kind {
        Kind::Part => schema_json::<Part<Scalar>>(),
        Kind::Feature => schema_json::<Feature<Scalar>>(),
        Kind::Geometry => schema_json::<Geometry<Scalar>>(),
        Kind::SketchLoop => schema_json::<SketchLoop<Scalar>>(),
        Kind::Selector => json!({
            "edge": schema_json::<EdgeSelector<Scalar>>(),
            "face": schema_json::<FaceSelector<Scalar>>(),
        }),
        Kind::Section => schema_json::<SectionAnalysis<Scalar>>(),
        Kind::Edit => schema_json::<Edit>(),
    }
}

fn schema_json<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("a schema is JSON")
}

/// Names the bad name and up to three near names. Without a near name, it gives the hint.
pub fn unknown(what: &str, name: &str, candidates: &[&str], hint: &str) -> String {
    let near = nearest(name, candidates);
    if near.is_empty() {
        format!("Unknown {what} {name}. {hint}")
    } else {
        format!("Unknown {what} {name}. Nearest: {}.", near.join(", "))
    }
}

/// Up to three candidates within a small edit distance, the closest first.
pub fn nearest<'a>(name: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let limit = (name.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .map(|candidate| (distance(name, candidate), *candidate))
        .filter(|(distance, _)| *distance <= limit)
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, name)| name).collect()
}

/// The Levenshtein distance between two names, counted in characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

fn variants(root: &Value) -> &[Value] {
    root["oneOf"]
        .as_array()
        .or_else(|| root["anyOf"].as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn op_name(variant: &Value) -> Option<&str> {
    let op = &variant["properties"]["op"];
    op["const"].as_str().or_else(|| op["enum"][0].as_str())
}

/// One line for each op: the name, then its fields. Optional fields end with `?`.
fn op_lines(root: &Value) -> Vec<String> {
    let defs = &root["$defs"];
    variants(root)
        .iter()
        .filter_map(|variant| {
            let name = op_name(variant)?;
            let required: Vec<&str> = variant["required"]
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let fields: Vec<String> = variant["properties"]
                .as_object()?
                .iter()
                .filter(|(key, _)| key.as_str() != "op")
                .map(|(key, schema)| field_text(key, schema, &required, defs))
                .collect();
            Some(if fields.is_empty() {
                name.to_string()
            } else {
                format!("{name}: {}", fields.join(", "))
            })
        })
        .collect()
}

fn field_text(name: &str, schema: &Value, required: &[&str], defs: &Value) -> String {
    let optional = if required.contains(&name) { "" } else { "?" };
    match type_text(schema, defs) {
        Some(kind) => format!("{name}{optional} ({kind})"),
        None => format!("{name}{optional}"),
    }
}

/// The short type of a field. Numbers and expressions give None, because they are the default.
fn type_text(schema: &Value, defs: &Value) -> Option<String> {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.rsplit('/').next().unwrap_or(reference);
        return match defs.get(name) {
            Some(def) if is_number_union(def) => None,
            _ => Some(snake_case(name)),
        };
    }
    if let Some(alternatives) = union(schema) {
        if is_number_union(schema) {
            return None;
        }
        let parts: Vec<String> = alternatives
            .iter()
            .filter(|alt| alt["type"].as_str() != Some("null"))
            .filter_map(|alt| type_text(alt, defs))
            .collect();
        return (!parts.is_empty()).then(|| parts.join(" | "));
    }
    let kind = match &schema["type"] {
        Value::String(kind) => kind.as_str(),
        Value::Array(kinds) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null")?,
        _ => return None,
    };
    match kind {
        "string" => Some("text".into()),
        "boolean" => Some("flag".into()),
        "array" => Some("list".into()),
        "object" => Some("object".into()),
        _ => None,
    }
}

fn union(schema: &Value) -> Option<&Vec<Value>> {
    schema["anyOf"]
        .as_array()
        .or_else(|| schema["oneOf"].as_array())
}

/// A union of numbers and text, such as an expression. It reads as a number.
fn is_number_union(schema: &Value) -> bool {
    let Some(alternatives) = union(schema) else {
        return false;
    };
    let kind = |alt: &Value| alt["type"].as_str().unwrap_or_default().to_string();
    alternatives.iter().any(|alt| kind(alt) == "number")
        && alternatives
            .iter()
            .all(|alt| matches!(kind(alt).as_str(), "number" | "string"))
}

fn snake_case(name: &str) -> String {
    let mut text = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if index > 0 {
                text.push('_');
            }
            text.extend(c.to_lowercase());
        } else {
            text.push(c);
        }
    }
    text
}

/// The schema of one op with the definitions it uses.
fn op_schema(root: &Value, op: &str) -> Result<Value, String> {
    let all = variants(root);
    let Some(variant) = all.iter().find(|variant| op_name(variant) == Some(op)) else {
        let names: Vec<&str> = all.iter().filter_map(op_name).collect();
        return Err(unknown(
            "op",
            op,
            &names,
            "Read the op list with kind geometry.",
        ));
    };
    let mut definition = variant.clone();
    let defs = used_definitions(variant, &root["$defs"]);
    if !defs.is_empty() {
        definition["$defs"] = Value::Object(defs);
    }
    Ok(json!({"schema":1,"kind":"geometry","op":op,"definition":definition}))
}

/// The definitions that the value uses, directly or through other definitions.
fn used_definitions<'a>(value: &'a Value, defs: &'a Value) -> Map<String, Value> {
    let mut seen = BTreeSet::new();
    let mut found = Map::new();
    let mut pending = vec![value];
    while let Some(node) = pending.pop() {
        match node {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                    let name = reference.rsplit('/').next().unwrap_or(reference);
                    if seen.insert(name.to_string())
                        && let Some(def) = defs.get(name)
                    {
                        found.insert(name.to_string(), def.clone());
                        pending.push(def);
                    }
                }
                pending.extend(map.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    found
}
