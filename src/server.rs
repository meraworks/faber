//! Stdio tools call the same project operations as the CLI.

use crate::input::Checked;
use crate::schema_tool;
use base64::Engine;
use faber_core::{PartInfo, Project, Reading};
use faber_model::{Edit, Feature, Geometry, Part, ProjectData, Scalar};
use faber_ports::Body;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const GUIDE: &str = include_str!("../docs/modelling-guide.md");

/// Short start-up instructions for the client. The guide has the details.
const INSTRUCTIONS: &str = "Faber builds editable solids. Units are millimetres and degrees. Z is up. Axes are right-handed.

Follow this loop for each change:
1. Call open with an absolute project path. Set create to true only for a new project.
2. Call status, params or part_info to read the current state.
3. Send the edits as one operations array to preview_batch. Nothing is saved.
4. Send the same operations to batch with expected_revision set to base_revision. Faber saves the change.
5. Call measure and part_info to check the result. Call render to see the model.

Call the schema tool to read the shape of a recipe. Use kind geometry with no op for the list of ops. Add op to read one op.
Call the guide tool to read the modelling guide. Call guide with no topic to list the topics.
A project with design.jsonnet is generated. Edit the source, then call build. Placement tools still work.
If a call fails, the error names the field and the JSON path. Correct that field and send the call again.";

/// A heading of the guide, with its line number and level.
struct Heading<'a> {
    line: usize,
    level: usize,
    text: &'a str,
}

/// Reads the headings of a Markdown guide. Lines in code fences are not headings.
fn guide_headings(markdown: &str) -> Vec<Heading<'_>> {
    let mut fenced = false;
    let mut headings = Vec::new();
    for (line, text) in markdown.lines().enumerate() {
        if text.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let level = text.chars().take_while(|c| *c == '#').count();
        if !fenced && (1..=6).contains(&level) && text[level..].starts_with(' ') {
            headings.push(Heading {
                line,
                level,
                text: text[level + 1..].trim(),
            });
        }
    }
    headings
}

/// The text of one topic: its heading and the lines up to the next heading of the same or higher level.
fn guide_section(markdown: &str, topic: &str) -> Option<String> {
    let headings = guide_headings(markdown);
    let index = headings
        .iter()
        .position(|heading| heading.level > 1 && heading.text.eq_ignore_ascii_case(topic))?;
    let start = headings[index].line;
    let end = headings[index + 1..]
        .iter()
        .find(|heading| heading.level <= headings[index].level)
        .map_or(markdown.lines().count(), |heading| heading.line);
    let lines: Vec<&str> = markdown.lines().collect();
    Some(lines[start..end].join("\n").trim_end().to_string())
}

/// Topics for the guide tool: every heading below the title.
fn guide_topics(markdown: &str) -> Vec<&str> {
    guide_headings(markdown)
        .into_iter()
        .filter(|heading| heading.level > 1)
        .map(|heading| heading.text)
        .collect()
}

#[derive(Clone)]
pub struct Faber {
    project: Arc<Mutex<Option<OpenProject>>>,
    tool_router: ToolRouter<Self>,
}

struct OpenProject {
    path: PathBuf,
    project: Project,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Open {
    path: String,
    #[serde(default)]
    create: bool,
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutPart {
    #[serde(default)]
    strict: bool,
    id: String,
    /// Part recipe. Read the shape with the schema tool, kind part.
    #[schemars(with = "serde_json::Value")]
    part: Part<Scalar>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportPart {
    purchased: Option<faber_model::Purchased>,
    id: String,
    name: String,
    path: String,
    #[serde(default)]
    strict: bool,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricFastener {
    /// Metric size, for example M3. Read the supported sizes in the guide.
    size: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPartInfo {
    /// Part id.
    id: String,
    /// New name. Must not be blank.
    name: Option<String>,
    /// New colour [R,G,B], 0 to 255.
    colour: Option<[u8; 3]>,
    /// New group name.
    group: Option<String>,
    /// New note.
    note: Option<String>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportArchive {
    path: String,
    destination: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MovePart {
    /// Part id.
    id: String,
    /// Move by [dx, dy, dz] in mm, relative to the current position.
    translate: Option<[f64; 3]>,
    /// Turn the part about an axis.
    rotate: Option<faber_core::Rotation>,
    /// Move by the vector from one world point to another.
    point_to_point: Option<faber_core::PointToPoint>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
    /// Move the bounding box centre to this world point [x, y, z] in mm.
    position: Option<[f64; 3]>,
    /// Keep the source part. Write the moved part as a new part with this id.
    copy_as: Option<String>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlignParts {
    /// Part that moves.
    id: String,
    /// Target part id, or "origin".
    target: String,
    /// Axes to align. Other axes do not move.
    axes: Vec<faber_core::Axis>,
    mode: faber_core::AlignMode,
    /// Gap in mm. Use it with min_to_max or max_to_min only.
    #[serde(default)]
    gap_mm: f64,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepMotion {
    joint: String,
    range: [f64; 2],
    #[serde(default)]
    proximity_mm: f64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MotionPose {
    #[serde(default)]
    values: BTreeMap<String, f64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutParts {
    #[serde(default)]
    strict: bool,
    /// Map from part id to part recipe. Read the shape with the schema tool, kind part.
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    parts: BTreeMap<String, Part<Scalar>>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParam {
    #[serde(default)]
    strict: bool,
    name: String,
    expression: String,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

/// Motion study input. Its fields sit beside expected_revision and verbose.
#[derive(JsonSchema)]
pub struct SetMotion {
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    verbose: bool,
    #[serde(flatten)]
    study: faber_model::motion::MotionStudy,
}

/// Reads expected_revision and verbose, then the study. The study refuses unknown fields.
/// Serde does not apply deny_unknown_fields across flatten, so this reads the parts itself.
impl<'de> Deserialize<'de> for SetMotion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut fields = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let expected_revision = match fields.remove("expected_revision") {
            None | Some(Value::Null) => None,
            Some(value) => Some(serde_json::from_value(value).map_err(D::Error::custom)?),
        };
        let verbose = match fields.remove("verbose") {
            None => false,
            Some(value) => serde_json::from_value(value).map_err(D::Error::custom)?,
        };
        let study = serde_json::from_value(Value::Object(fields)).map_err(D::Error::custom)?;
        Ok(Self {
            expected_revision,
            verbose,
            study,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreCheckpoint {
    name: String,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interference {
    proximity_mm: f64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetContact {
    /// First part id.
    a: String,
    /// Second part id.
    b: String,
    /// True declares the contact. False removes it.
    allowed: bool,
    /// Short reason for the contact. Up to 500 characters.
    #[serde(default)]
    note: String,
    /// Deepest allowed overlap in millimetres. Zero allows touch only. Default 0.6.
    #[serde(default)]
    max_depth_mm: Option<f64>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSection {
    /// Section id. Up to 100 letters, digits, `_` or `-`.
    id: String,
    /// The section. It names a plane, a reference feature or a planar face.
    section: faber_model::SectionAnalysis<Scalar>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionId {
    /// Section id.
    id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSection {
    /// Section id.
    id: String,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    a: String,
    b: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassProperties {
    /// Part id.
    id: String,
    /// Density in g/cm3. Must be above zero.
    density_g_cm3: Option<f64>,
}

/// Overhang input. Without a part, every part is analysed.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverhangAnalysis {
    /// Part id.
    part: Option<String>,
    /// Build direction. Default is up, [0, 0, 1].
    #[serde(default = "default_build_direction")]
    direction: [f64; 3],
    /// Angle from the downward direction. Faces within this angle need support. From 0 to 90.
    #[serde(default = "default_critical_angle")]
    critical_angle_deg: f64,
}

fn default_build_direction() -> [f64; 3] {
    [0., 0., 1.]
}

fn default_critical_angle() -> f64 {
    45.
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    /// Ordered operations. Each has a tool field. Read the shapes with the schema tool, kind edit.
    #[schemars(with = "Vec<serde_json::Value>")]
    operations: Vec<Value>,
    expected_revision: Option<u64>,
    #[serde(default)]
    strict: bool,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewBatch {
    /// Ordered operations. Each has a tool field. Read the shapes with the schema tool, kind edit.
    #[schemars(with = "Vec<serde_json::Value>")]
    operations: Vec<Value>,
    expected_revision: Option<u64>,
    #[serde(default)]
    strict: bool,
    render: Option<Render>,
}

/// Batch tool names, in the order shown to clients.
const BATCH_TOOLS: [&str; 15] = [
    "set_motion",
    "set_section",
    "remove_section",
    "set_part_info",
    "set_param",
    "put_part",
    "remove_part",
    "add_feature",
    "edit_feature",
    "delete_feature",
    "move_feature",
    "move_part",
    "align_parts",
    "suppress_feature",
    "set_contact",
];

/// A move item in a batch. Its fields are the move_part arguments without expected_revision.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveItem {
    id: String,
    translate: Option<[f64; 3]>,
    rotate: Option<faber_core::Rotation>,
    point_to_point: Option<faber_core::PointToPoint>,
    position: Option<[f64; 3]>,
    copy_as: Option<String>,
}

/// An align item in a batch. Its fields are the align_parts arguments without expected_revision.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlignItem {
    id: String,
    target: String,
    axes: Vec<faber_core::Axis>,
    mode: faber_core::AlignMode,
    #[serde(default)]
    gap_mm: f64,
}

/// Reads the fields of a set_part_info item. Serde does not name
/// the field in a type error, so each field is read here.
fn part_info(fields: serde_json::Map<String, Value>) -> Result<PartInfo, String> {
    let mut info = PartInfo::default();
    let mut has_id = false;
    for (key, value) in fields {
        let read = |e: serde_json::Error| format!("field `{key}`: {e}");
        match key.as_str() {
            "id" => {
                info.id = serde_json::from_value(value).map_err(read)?;
                has_id = true;
            }
            "name" => info.name = serde_json::from_value(value).map_err(read)?,
            "colour" => info.colour = serde_json::from_value(value).map_err(read)?,
            "group" => info.group = serde_json::from_value(value).map_err(read)?,
            "note" => info.note = serde_json::from_value(value).map_err(read)?,
            _ => {
                return Err(format!(
                    "unknown field `{key}`, expected one of `id`, `name`, `colour`, `group`, `note`"
                ));
            }
        }
    }
    if !has_id {
        return Err("missing field `id`".into());
    }
    Ok(info)
}

/// Reads each batch item by its tool field. Errors name the item.
fn batch_operations(items: Vec<Value>) -> Result<Vec<faber_core::Operation>, String> {
    let accepted = BATCH_TOOLS.join(", ");
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let number = index + 1;
            let Value::Object(mut fields) = item else {
                return Err(format!(
                    "Batch operation {number}: expected an object with a tool field. Accepted tools: {accepted}."
                ));
            };
            let tool = match fields.get("tool") {
                None => {
                    return Err(format!(
                        "Batch operation {number}: missing field `tool`. Accepted tools: {accepted}."
                    ));
                }
                Some(Value::String(name)) => name.clone(),
                Some(_) => {
                    return Err(format!(
                        "Batch operation {number}: field `tool` must be a text value. Accepted tools: {accepted}."
                    ));
                }
            };
            if tool == "set_part_info" {
                fields.remove("tool");
                let info = part_info(fields)
                    .map_err(|e| format!("Batch operation {number} (tool `{tool}`): {e}"))?;
                return Ok(faber_core::Operation::set_part_info(info));
            }
            if !BATCH_TOOLS.contains(&tool.as_str()) {
                let known = schema_tool::unknown("tool", &tool, &BATCH_TOOLS, "");
                return Err(format!(
                    "Batch operation {number}: {} Accepted tools: {accepted}.",
                    known.trim_end()
                ));
            }
            let path = format!("operations[{index}]");
            if tool == "move_part" {
                fields.remove("tool");
                let item = crate::input::read::<MoveItem>(&Value::Object(fields), &path)
                    .map_err(|e| format!("Batch operation {number} (tool `{tool}`): {e}"))?;
                let motion = faber_core::Motion::from_options(
                    item.translate,
                    item.rotate,
                    item.point_to_point,
                    item.position,
                )
                .map_err(|e| format!("Batch operation {number} (tool `{tool}`): {e}"))?;
                return Ok(faber_core::Operation::Move {
                    id: item.id,
                    motion,
                    copy_as: item.copy_as,
                });
            }
            if tool == "align_parts" {
                fields.remove("tool");
                let item = crate::input::read::<AlignItem>(&Value::Object(fields), &path)
                    .map_err(|e| format!("Batch operation {number} (tool `{tool}`): {e}"))?;
                return Ok(faber_core::Operation::Align {
                    id: item.id,
                    target: item.target,
                    axes: item.axes,
                    mode: item.mode,
                    gap_mm: item.gap_mm,
                });
            }
            if tool == "suppress_feature" {
                // Read as the set_suppressed edit. Suppression is the default, as in the tool.
                fields.insert("tool".into(), json!("set_suppressed"));
                fields.entry("suppressed").or_insert(json!(true));
            }
            let edit = crate::input::read::<Edit>(&Value::Object(fields), &path)
            .map_err(|e| format!("Batch operation {number} (tool `{tool}`): {e}"))?;
            Ok(faber_core::Operation::from(edit))
        })
        .collect()
}

/// The parts before a change. It finds the parts that the change touched.
struct Before {
    parts: BTreeMap<String, Part<Scalar>>,
    summaries: BTreeMap<String, Value>,
}

impl Before {
    fn of(project: &Project) -> Self {
        Self {
            parts: project.data.parts.clone(),
            summaries: project
                .bodies
                .iter()
                .map(|body| (body.id.clone(), part_summary(body)))
                .collect(),
        }
    }

    /// Summaries of the parts whose recipe or solid changed.
    fn changed(&self, project: &Project) -> Vec<Value> {
        project
            .bodies
            .iter()
            .filter_map(|body| {
                let summary = part_summary(body);
                let differs = self.parts.get(&body.id) != project.data.parts.get(&body.id)
                    || self.summaries.get(&body.id) != Some(&summary);
                differs.then_some(summary)
            })
            .collect()
    }
}

fn part_summary(body: &Body) -> Value {
    json!({"id":body.id,"volume_mm3":body.volume,"bounds_mm":body.bounds,"triangles":body.mesh.triangles.len()})
}

/// The reply of a changing tool. It lists the changed parts. Verbose gives every part.
fn changed_reply(before: &Before, project: &Project, verbose: bool) -> Value {
    if verbose {
        return report(&project.data, &project.bodies);
    }
    json!({"schema":1,"revision":project.data.revision,"changed":before.changed(project),"part_count":project.bodies.len()})
}

fn check_revision(project: &Project, expected: Option<u64>) -> Result<(), String> {
    if let Some(expected) = expected
        && expected != project.data.revision
    {
        return Err(format!(
            "Project revision changed: expected {expected}, current {}",
            project.data.revision
        ));
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddFeature {
    #[serde(default)]
    strict: bool,
    part: String,
    /// Feature recipe. Read the shape with the schema tool, kind feature.
    #[schemars(with = "serde_json::Value")]
    feature: Feature<Scalar>,
    before: Option<String>,
    #[serde(default)]
    make_output: bool,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditFeature {
    #[serde(default)]
    strict: bool,
    part: String,
    id: String,
    /// Geometry of the feature. Read the shape with the schema tool, kind geometry.
    #[schemars(with = "serde_json::Value")]
    geometry: Geometry<Scalar>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureId {
    #[serde(default)]
    strict: bool,
    part: String,
    id: String,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveFeature {
    #[serde(default)]
    strict: bool,
    part: String,
    id: String,
    before: Option<String>,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

/// Feature read input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureInfoArgs {
    /// Part id.
    part: String,
    /// Feature id in the part history.
    feature: String,
}

/// How part_info lists the features of a history.
#[derive(Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FeatureView {
    /// Every feature recipe.
    #[default]
    Full,
    /// Only the feature ids.
    Ids,
}

/// Part read input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartInfoArgs {
    id: String,
    /// "full" (default) gives every feature recipe. "ids" gives only the feature ids.
    #[serde(default)]
    features: FeatureView,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideArgs {
    /// A topic from the list. Omit it to list the topics.
    topic: Option<String>,
}

fn suppress_by_default() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SuppressFeature {
    #[serde(default)]
    strict: bool,
    part: String,
    id: String,
    /// True suppresses the feature. False unsuppresses it.
    #[serde(default = "suppress_by_default")]
    suppressed: bool,
    /// With suppressed true, also suppress the features that depend on it.
    #[serde(default)]
    cascade: bool,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    id: String,
    feature: Option<String>,
    kind: Option<String>,
    edge: Option<faber_model::EdgeSelector>,
    face: Option<faber_model::FaceSelector>,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePart {
    id: String,
    #[serde(default)]
    strict: bool,
    /// Refuse the change if the project revision differs.
    expected_revision: Option<u64>,
    /// Return every part, not only the changed parts.
    #[serde(default)]
    verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Export {
    directory: String,
    #[serde(default)]
    force: bool,
    #[serde(default = "default_format")]
    format: String,
}

fn default_format() -> String {
    "stl".into()
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Render {
    path: String,
    part: Option<String>,
    #[serde(default)]
    view: faber_scene::View,
    section: Option<SectionArg>,
    #[serde(default = "image_width")]
    width: u32,
    #[serde(default = "image_height")]
    height: u32,
    /// Send the image in the reply. The file is always written.
    #[serde(default)]
    inline: bool,
}

/// A section for render. It is a saved section id or an inline plane.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SectionArg {
    /// Id of a saved section.
    Saved(String),
    /// A plane in world coordinates.
    Plane(faber_scene::Section),
}

/// The section and the bodies to draw. A saved section with parts draws only those parts.
fn render_input(
    project: &Project,
    render: &Render,
) -> Result<(Option<faber_scene::Section>, Vec<Body>), String> {
    let (section, parts) = match &render.section {
        None => (None, Vec::new()),
        Some(SectionArg::Plane(section)) => (Some(*section), Vec::new()),
        Some(SectionArg::Saved(id)) => {
            let saved = project.section(id)?;
            let offset = (0..3).map(|i| saved.normal[i] * saved.origin[i]).sum();
            let section = faber_scene::Section {
                normal: saved.normal,
                offset,
                reverse: saved.reverse,
            };
            (Some(section), saved.parts)
        }
    };
    let bodies = render
        .bodies(&project.bodies)?
        .iter()
        .filter(|body| parts.is_empty() || parts.contains(&body.id))
        .cloned()
        .collect();
    Ok((section, bodies))
}

impl Render {
    fn bodies<'a>(&self, bodies: &'a [Body]) -> Result<&'a [Body], String> {
        match &self.part {
            None => Ok(bodies),
            Some(id) => bodies
                .iter()
                .find(|body| body.id == *id)
                .map(std::slice::from_ref)
                .ok_or_else(|| format!("Unknown render part: {id}")),
        }
    }
}
fn image_width() -> u32 {
    1200
}
fn image_height() -> u32 {
    1000
}

pub fn report(data: &ProjectData, bodies: &[Body]) -> Value {
    json!({"schema":1,"name":data.name,"revision":data.revision,"part_count":bodies.len(),"parts":bodies.iter().map(|b| json!({"id":b.id,"name":b.name,"group":b.group,"volume_mm3":b.volume,"bounds_mm":b.bounds,"triangles":b.mesh.triangles.len()})).collect::<Vec<_>>()})
}

/// The reply of a placement tool. Each changed part also gives its placement.
fn placed_reply(before: &Before, project: &Project, verbose: bool) -> Value {
    let mut reply = changed_reply(before, project, verbose);
    if let Some(changed) = reply["changed"].as_array_mut() {
        for entry in changed {
            let id = entry["id"].as_str().unwrap_or_default().to_string();
            entry["placement"] = json!(project.data.parts.get(&id).map(|part| &part.placement));
        }
    }
    reply
}

/// Add the resolved absolute project path to the open reply.
fn open_reply(project: &Project, path: &Path) -> Value {
    let mut value = report(&project.data, &project.bodies);
    value["path"] = json!(display_path(path));
    value["source"] = json!(project.source());
    with_source_error(value, project)
}

/// Adds `source_error` to a reply when the last design source build failed.
/// The reply then describes the last good state.
fn with_source_error(mut value: Value, project: &Project) -> Value {
    if let (Some(error), Some(map)) = (project.source_error(), value.as_object_mut()) {
        map.insert("source_error".into(), json!(error));
    }
    value
}

/// Remove the Windows verbatim prefix from a resolved path.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    }
}

fn result(value: Result<Value, String>) -> CallToolResult {
    match value {
        Ok(value) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]),
    }
}

impl Faber {
    pub fn new() -> Self {
        Self {
            project: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    fn with(&self, op: impl FnOnce(&mut Project) -> Result<Value, String>) -> CallToolResult {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        result((|| {
            let open = guard.as_mut().ok_or("Open a project first")?;
            faber_wiring::refresh(&open.path, &mut open.project)?;
            let value = op(&mut open.project)?;
            Ok(with_source_error(value, &open.project))
        })())
    }

    /// Like `with`, and the operation also gets the project folder. A history step needs it.
    fn with_folder(
        &self,
        op: impl FnOnce(&Path, &mut Project) -> Result<Value, String>,
    ) -> CallToolResult {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        result((|| {
            let open = guard.as_mut().ok_or("Open a project first")?;
            faber_wiring::refresh(&open.path, &mut open.project)?;
            let value = op(&open.path, &mut open.project)?;
            Ok(with_source_error(value, &open.project))
        })())
    }
}

#[tool_router]
impl Faber {
    #[tool(
        description = "Open a project. Use an absolute path. Set create to true to create a project if it does not exist."
    )]
    fn open(&self, Parameters(Checked(args)): Parameters<Checked<Open>>) -> CallToolResult {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        result((|| {
            let requested = Path::new(&args.path);
            if !requested.is_absolute() {
                return Err("Use an absolute path, for example C:\\work\\part.".to_string());
            }
            if let Some(current) = guard.as_mut()
                && std::fs::canonicalize(requested).ok().as_ref() == Some(&current.path)
            {
                faber_wiring::refresh(&current.path, &mut current.project)?;
                return Ok(open_reply(&current.project, &current.path));
            }
            let project = faber_wiring::open(requested, args.create, args.name)?;
            let path = std::fs::canonicalize(requested).map_err(|e| e.to_string())?;
            let value = open_reply(&project, &path);
            *guard = Some(OpenProject { path, project });
            Ok(value)
        })())
    }

    #[tool(
        description = "Measure minimum distance between two placed parts using their exact shapes. Return millimetres and one pair of closest points in world coordinates. Zero can mean contact or overlap; this is not penetration depth. The project stays unchanged."
    )]
    fn measure(&self, Parameters(Checked(args)): Parameters<Checked<Measure>>) -> CallToolResult {
        self.with(|p| {
            let measured = p.measure_distance(&args.a, &args.b)?;
            Ok(
                json!({"schema":1,"revision":p.data.revision,"a":args.a,"b":args.b,
                "coordinates":"world","distance_mm":measured.distance_mm,
                "point_a":measured.point_a,"point_b":measured.point_b}),
            )
        })
    }

    #[tool(
        description = "Read the volume, surface area and centre of mass of a placed part in world coordinates. If density_g_cm3 is set, also read the mass and the inertia about the centre of mass. The project stays unchanged."
    )]
    fn mass_properties(
        &self,
        Parameters(Checked(args)): Parameters<Checked<MassProperties>>,
    ) -> CallToolResult {
        self.with(|p| {
            let report = p.mass_properties(&args.id, args.density_g_cm3)?;
            let mut value = json!({"schema":1,"revision":p.data.revision,"id":args.id,
                "coordinates":"world","volume_mm3":report.volume_mm3,"area_mm2":report.area_mm2,
                "centre_of_mass_mm":report.centre_of_mass_mm});
            if let Some(mass_g) = report.mass_g {
                value["mass_g"] = json!(mass_g);
            }
            if let Some(inertia) = report.inertia_kg_mm2 {
                value["inertia_kg_mm2"] = json!(inertia);
            }
            Ok(value)
        })
    }

    #[tool(description = "Read part bounds, volume and triangle counts.")]
    fn status(&self) -> CallToolResult {
        self.with(|p| {
            let mut value = report(&p.data, &p.bodies);
            value["source"] = json!(p.source());
            Ok(value)
        })
    }

    #[tool(
        description = "Build the design source of the open project. Read the written files and the added or removed parts. Placement of kept parts stays."
    )]
    fn build(&self) -> CallToolResult {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        result((|| {
            let open = guard.as_mut().ok_or("Open a project first")?;
            let built = faber_wiring::build_source(&open.path, &mut open.project)?;
            let mut value = serde_json::to_value(built).map_err(|e| e.to_string())?;
            value["schema"] = json!(1);
            value["revision"] = json!(open.project.data.revision);
            Ok(value)
        })())
    }

    #[tool(
        description = "Check placed bodies for mesh interference and clearance. Depth is approximate. Pairs that touch are reported as contact and do not fail. Declared contacts give no violation. This read-only check does not change the design."
    )]
    fn interference(
        &self,
        Parameters(Checked(args)): Parameters<Checked<Interference>>,
    ) -> CallToolResult {
        self.with(|p| {
            let report =
                faber_wiring::interference(&p.bodies, args.proximity_mm, &p.data.contacts)?;
            Ok(json!({"schema":1,"revision":p.data.revision,"report":report}))
        })
    }

    #[tool(
        description = "Apply ordered design edits as one transaction. Build the final state once. Any failure refuses the whole batch. Set expected_revision to refuse a stale edit. Each operation has a tool field and its arguments."
    )]
    fn batch(&self, Parameters(Checked(args)): Parameters<Checked<Batch>>) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.batch_operations(batch_operations(args.operations)?, args.strict)?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Build a candidate from ordered edits without saving or changing undo history. Return its bounds and volumes with the base revision. Optional render accepts path, width, height and section and returns a PNG image. To save the design, send the same operations to batch with expected_revision set to base_revision. Strict checks are optional."
    )]
    fn preview_batch(
        &self,
        Parameters(Checked(args)): Parameters<Checked<PreviewBatch>>,
    ) -> CallToolResult {
        let mut image = None;
        let mut response = self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let base_revision = p.data.revision;
            let (data, bodies) = p.preview_operations(batch_operations(args.operations)?, args.strict)?;
            if let Some(render) = args.render {
                let section = match &render.section {
                    None => None,
                    Some(SectionArg::Plane(section)) => Some(*section),
                    Some(SectionArg::Saved(_)) => {
                        return Err("Preview renders take an inline section. Save the section to render it by id.".into());
                    }
                };
                faber_wiring::render_view(
                    render.bodies(&bodies)?,
                    Path::new(&render.path),
                    section,
                    render.width,
                    render.height,
                    render.view,
                )?;
                image = Some(std::fs::read(&render.path).map_err(|e| e.to_string())?);
            }
            Ok(json!({"schema":1,"saved":false,"base_revision":base_revision,"candidate":report(&data,&bodies)}))
        });
        if let Some(bytes) = image {
            response.content.push(ContentBlock::image(
                base64::prelude::BASE64_STANDARD.encode(bytes),
                "image/png",
            ));
        }
        response
    }

    #[tool(
        description = "Save the current design under a unique name. Names are case insensitive."
    )]
    fn checkpoint(
        &self,
        Parameters(Checked(args)): Parameters<Checked<Checkpoint>>,
    ) -> CallToolResult {
        self.with(|p| {
            p.checkpoint(&args.name)?;
            Ok(report(&p.data, &p.bodies))
        })
    }

    #[tool(
        description = "Import STEP, STL, 3MF, OBJ, PLY or OFF as a new part. Use an absolute source path. Keep mesh faces and preserve the source bytes in the project. Files without units use millimetres. Mesh parts do not have exact face or solid operations. Materials and textures are not imported. This is not a purchased-part record."
    )]
    fn import_part(
        &self,
        Parameters(Checked(args)): Parameters<Checked<ImportPart>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let path = Path::new(&args.path);
            if !path.is_absolute() {
                return Err("Import needs an absolute source path".into());
            }
            let before = Before::of(p);
            p.import_with_record(args.id, args.name, path, args.strict, args.purchased)?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Restore a named checkpoint. Build before save. Undo can reverse this change."
    )]
    fn restore(
        &self,
        Parameters(Checked(args)): Parameters<Checked<RestoreCheckpoint>>,
    ) -> CallToolResult {
        self.with_folder(|path, p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            faber_wiring::restore(path, p, &args.name)?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Undo the last saved change. Refuse if the current file differs from its recorded state."
    )]
    fn undo(&self, Parameters(Checked(args)): Parameters<Checked<Revision>>) -> CallToolResult {
        self.with_folder(|path, p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            faber_wiring::history_move(path, p, faber_ports::HistoryMove::Undo)?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Store the as-built assembly joints, gear ratios and connecting rods. Coordinates are in the assembled world frame. Angles are in degrees and distances are in millimetres."
    )]
    fn set_motion(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SetMotion>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.batch(vec![faber_model::Edit::SetMotion { study: args.study }])?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Check interference throughout one joint range. Other independent drives stay at zero. An uncertain interval needs further inspection."
    )]
    fn sweep_motion(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SweepMotion>>,
    ) -> CallToolResult {
        self.with(|p| {
            Ok(json!(faber_wiring::motion_check::sweep(
                &p.bodies,
                &p.data.motion,
                &p.data.contacts,
                &args.joint,
                args.range,
                args.proximity_mm
            )?))
        })
    }

    #[tool(
        description = "Import a local Fusion archive into a new editable project folder. Start the FaberArchive add-in in Fusion first."
    )]
    fn import_archive(
        &self,
        Parameters(Checked(args)): Parameters<Checked<ImportArchive>>,
    ) -> CallToolResult {
        result((|| {
            let source = Path::new(&args.path);
            let dest = Path::new(&args.destination);
            if !source.is_absolute() || !dest.is_absolute() {
                return Err("Supply two absolute paths".into());
            }
            faber_wiring::archive::import(source, dest)?;
            Ok(json!({"path":args.destination}))
        })())
    }

    #[tool(
        description = "Solve a pose without changing the project. Supply independent joint values. Returns world transforms relative to the as-built position."
    )]
    fn motion_pose(
        &self,
        Parameters(Checked(args)): Parameters<Checked<MotionPose>>,
    ) -> CallToolResult {
        self.with(|p| Ok(json!({"transforms":p.data.motion.solve(&args.values)?})))
    }

    #[tool(description = "Redo a saved change after undo. A new edit clears the redo branch.")]
    fn redo(&self, Parameters(Checked(args)): Parameters<Checked<Revision>>) -> CallToolResult {
        self.with_folder(|path, p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            faber_wiring::history_move(path, p, faber_ports::HistoryMove::Redo)?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Read exact edges and faces of a placed part in world coordinates. Set feature to a history feature id to query its cached shape in part coordinates before part placement. Use this source query to set feature selectors. Indices are valid only for the returned revision and shape. Set kind to edges or faces to return one list. Set edge or face to a selector that filters the lists; its expect is not checked. Set limit to cap each list. Filtered replies add total counts before the limit."
    )]
    fn topology(&self, Parameters(Checked(args)): Parameters<Checked<Topology>>) -> CallToolResult {
        self.with(|p| {
            let kind = args.kind.as_deref();
            if !matches!(kind, None | Some("edges") | Some("faces")) {
                return Err("Kind must be edges or faces".into());
            }
            if kind == Some("faces") && args.edge.is_some() {
                return Err("Kind faces takes a face selector, not an edge selector".into());
            }
            if kind == Some("edges") && args.face.is_some() {
                return Err("Kind edges takes an edge selector, not a face selector".into());
            }
            if args.limit == Some(0) {
                return Err("Limit must be at least 1".into());
            }
            let filtered = kind.is_some()
                || args.edge.is_some()
                || args.face.is_some()
                || args.limit.is_some();
            let (mut edges, mut faces) = p.selected_topology(
                &args.id,
                args.feature.as_deref(),
                args.edge.as_ref(),
                args.face.as_ref(),
            )?;
            let total = json!({"edges":edges.len(),"faces":faces.len()});
            if kind == Some("faces") {
                edges.clear();
            }
            if kind == Some("edges") {
                faces.clear();
            }
            if let Some(limit) = args.limit {
                edges.truncate(limit);
                faces.truncate(limit);
            }
            let coordinates = if args.feature.is_some() { "part" } else { "world" };
            let mut reply = json!({"schema":1,"id":args.id,"feature":args.feature,"coordinates":coordinates,"revision":p.data.revision,"edges":edges,"faces":faces});
            if filtered {
                reply["total"] = total;
            }
            Ok(reply)
        })
    }

    #[tool(
        description = "Read a part recipe, ordered features, placement, bounds and volume. Set features to ids to read only the feature ids."
    )]
    fn part_info(
        &self,
        Parameters(Checked(args)): Parameters<Checked<PartInfoArgs>>,
    ) -> CallToolResult {
        self.with(|p| {
            let recipe = p.data.parts.get(&args.id).ok_or_else(|| format!("Unknown part: {}", args.id))?;
            let body = p.bodies.iter().find(|b| b.id == args.id).ok_or("Part has no built body")?;
            let mut recipe = serde_json::to_value(recipe).map_err(|e| e.to_string())?;
            if args.features == FeatureView::Ids
                && let Some(Value::Array(features)) = recipe.pointer_mut("/geometry/features")
            {
                let ids: Vec<Value> = features.iter().map(|feature| feature["id"].clone()).collect();
                *features = ids;
            }
            Ok(json!({"schema":1,"id":args.id,"recipe":recipe,"bounds_mm":body.bounds,"volume_mm3":body.volume,"revision":p.data.revision}))
        })
    }

    #[tool(
        description = "Read one feature of a part in part coordinates. A solid gives volume, bounds, faces, edges, suppression and depends_on. A plane, axis or point gives its frame. The project stays unchanged."
    )]
    fn feature_info(
        &self,
        Parameters(Checked(args)): Parameters<Checked<FeatureInfoArgs>>,
    ) -> CallToolResult {
        self.with(|p| {
            let info = p.feature_info(&args.part, &args.feature)?;
            let mut reply = json!({"schema":1,"part":args.part,"feature":args.feature,"kind":info.kind,"revision":p.data.revision});
            match info.reading {
                Reading::Solid { volume_mm3, bounds_mm, faces, edges, suppressed, depends_on } => {
                    reply["volume_mm3"] = json!(volume_mm3);
                    reply["bounds_mm"] = json!(bounds_mm);
                    reply["faces"] = json!(faces);
                    reply["edges"] = json!(edges);
                    reply["suppressed"] = json!(suppressed);
                    reply["depends_on"] = json!(depends_on);
                }
                Reading::Plane { origin, x, y, normal } => {
                    reply["origin"] = json!(origin);
                    reply["x"] = json!(x);
                    reply["y"] = json!(y);
                    reply["normal"] = json!(normal);
                }
                Reading::Axis { origin, direction } => {
                    reply["origin"] = json!(origin);
                    reply["direction"] = json!(direction);
                }
                Reading::Suppressed => reply["suppressed"] = json!(true),
                Reading::Point { point } => reply["point"] = json!(point),
            }
            Ok(reply)
        })
    }

    #[tool(description = "Read shared parameter expressions and calculated values.")]
    fn params(&self) -> CallToolResult {
        self.with(|p| Ok(json!({"schema":1,"expressions":p.data.params,"values":p.params()?})))
    }

    #[tool(
        description = "Insert a feature before an id, or append it. Set make_output to use its solid as the part output."
    )]
    fn add_feature(
        &self,
        Parameters(Checked(args)): Parameters<Checked<AddFeature>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::AddFeature {
                    part: args.part,
                    feature: args.feature,
                    before: args.before,
                    make_output: args.make_output,
                }])?;
            } else {
                p.add_feature(
                    &args.part,
                    args.feature,
                    args.before.as_deref(),
                    args.make_output,
                )?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Replace a feature operation and rebuild its dependent results before save."
    )]
    fn edit_feature(
        &self,
        Parameters(Checked(args)): Parameters<Checked<EditFeature>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::EditFeature {
                    part: args.part,
                    id: args.id,
                    geometry: args.geometry,
                }])?;
            } else {
                p.edit_feature(&args.part, &args.id, args.geometry)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Delete a feature. Refuse deletion if a later operation or the output still needs it."
    )]
    fn delete_feature(
        &self,
        Parameters(Checked(args)): Parameters<Checked<FeatureId>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::DeleteFeature {
                    part: args.part,
                    id: args.id,
                }])?;
            } else {
                p.delete_feature(&args.part, &args.id)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Move a feature before an id, or to the end. Refuse an order with invalid dependencies."
    )]
    fn move_feature(
        &self,
        Parameters(Checked(args)): Parameters<Checked<MoveFeature>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::MoveFeature {
                    part: args.part,
                    id: args.id,
                    before: args.before,
                }])?;
            } else {
                p.move_feature(&args.part, &args.id, args.before.as_deref())?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Suppress or unsuppress a feature. A suppressed feature stays in the recipe. A suppressed modifier, for example a fillet or a hole, passes its input solid to later features. A suppressed creation feature gives no solid: features that need it are refused unless cascade is true. The change is one undo step."
    )]
    fn suppress_feature(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SuppressFeature>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::SetSuppressed {
                    part: args.part,
                    id: args.id,
                    suppressed: args.suppressed,
                    cascade: args.cascade,
                }])?;
            } else {
                p.set_suppressed(&args.part, &args.id, args.suppressed, args.cascade)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(description = "Set a shared parameter expression. Rebuild affected solids before save.")]
    fn set_param(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SetParam>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            let (name, expression) = (args.name.clone(), args.expression.clone());
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::SetParam {
                    name: args.name,
                    expression: args.expression,
                }])?;
            } else {
                p.set_params(BTreeMap::from([(args.name, args.expression)]))?;
            }
            let param = json!({"name":name,"expression":expression,"value":p.params()?.get(&name)});
            if args.verbose {
                let mut reply = report(&p.data, &p.bodies);
                reply["param"] = param;
                return Ok(reply);
            }
            Ok(json!({"revision":p.data.revision,"param":param,"changed":before.changed(p)}))
        })
    }

    #[tool(description = "Add or replace a part. Build the solid before saving the project.")]
    fn put_part(&self, Parameters(Checked(args)): Parameters<Checked<PutPart>>) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::PutPart {
                    id: args.id,
                    part: args.part,
                }])?;
            } else {
                p.put_part(args.id, args.part)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Change the name, colour, group or note of a part. Set at least one field. The geometry, placement and volume do not change. Each change makes one undo step."
    )]
    fn set_part_info(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SetPartInfo>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.set_part_info(PartInfo {
                id: args.id,
                name: args.name,
                colour: args.colour,
                group: args.group,
                note: args.note,
            })?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Add or replace several parts as one change. Refuse the whole change if any solid fails."
    )]
    fn put_parts(
        &self,
        Parameters(Checked(args)): Parameters<Checked<PutParts>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(
                    args.parts
                        .into_iter()
                        .map(|(id, part)| faber_model::Edit::PutPart { id, part })
                        .collect(),
                )?;
            } else {
                p.put_parts(args.parts)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Move or turn a part. Give exactly one mode: translate, rotate, point_to_point or position. Set copy_as to keep the source part. In rotate, origin is the world origin. Reply lists the changed parts with placement and bounds. The change is one undo step."
    )]
    fn move_part(
        &self,
        Parameters(Checked(args)): Parameters<Checked<MovePart>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let motion = faber_core::Motion::from_options(
                args.translate,
                args.rotate,
                args.point_to_point,
                args.position,
            )?;
            let before = Before::of(p);
            p.move_part(&args.id, motion, args.copy_as.as_deref())?;
            Ok(placed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Move a part so its bounding box lines up with a target part or with the origin. Translation only. Use mode min_to_max to stack a part on its target. Reply lists the changed parts with placement and bounds. The change is one undo step."
    )]
    fn align_parts(
        &self,
        Parameters(Checked(args)): Parameters<Checked<AlignParts>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.align_part(&args.id, &args.target, &args.axes, args.mode, args.gap_mm)?;
            Ok(placed_reply(&before, p, args.verbose))
        })
    }

    #[tool(description = "Remove a part by its id.")]
    fn remove_part(
        &self,
        Parameters(Checked(args)): Parameters<Checked<RemovePart>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            if args.strict {
                p.batch_strict(vec![faber_model::Edit::RemovePart { id: args.id }])?;
            } else {
                p.remove_part(&args.id)?;
            }
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Declare or remove an intended contact of two parts. An allowed pair can touch or press up to max_depth_mm. A deeper overlap is a violation. Set allowed to false to remove the contact."
    )]
    fn set_contact(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SetContact>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.batch_operations(
                vec![faber_core::Operation::from(faber_model::Edit::SetContact {
                    a: args.a,
                    b: args.b,
                    allowed: args.allowed,
                    note: args.note,
                    max_depth_mm: args.max_depth_mm,
                })],
                false,
            )?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Add or replace a saved section analysis. The plane can be a plane, a reference feature or a planar face. Set visible to false to hide it in views. A section never changes a solid. Each change makes one undo step."
    )]
    fn set_section(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SetSection>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.batch_operations(
                vec![faber_core::Operation::from(faber_model::Edit::SetSection {
                    id: args.id,
                    section: args.section,
                })],
                false,
            )?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Remove a saved section analysis by its id. The solids do not change. Undo can reverse this change."
    )]
    fn remove_section(
        &self,
        Parameters(Checked(args)): Parameters<Checked<RemoveSection>>,
    ) -> CallToolResult {
        self.with(|p| {
            check_revision(p, args.expected_revision)?;
            let before = Before::of(p);
            p.batch_operations(
                vec![faber_core::Operation::from(
                    faber_model::Edit::RemoveSection { id: args.id },
                )],
                false,
            )?;
            Ok(changed_reply(&before, p, args.verbose))
        })
    }

    #[tool(
        description = "Read the exact cut of each cut part by a saved section. Read the plane origin and normal in world coordinates. Read the cut area, perimeter, 2D bounds, regions and holes in plane axes. min_wall_mm is approximate. The project stays unchanged."
    )]
    fn section_info(
        &self,
        Parameters(Checked(args)): Parameters<Checked<SectionId>>,
    ) -> CallToolResult {
        self.with(|p| {
            let info = p.section_info(&args.id)?;
            Ok(json!({"schema":1,"id":args.id,"revision":p.data.revision,"coordinates":"world",
                "origin_mm":info.origin_mm,"normal":info.normal,"axes":info.axes,"parts":info.parts}))
        })
    }

    #[tool(
        description = "Find faces that need support for 3D printing. Each face is classed by its angle to the build direction. Read the support area, the bed area and the flattest overhang angle. Read up to five largest support regions with centroid and area. The project stays unchanged."
    )]
    fn overhang_analysis(
        &self,
        Parameters(Checked(args)): Parameters<Checked<OverhangAnalysis>>,
    ) -> CallToolResult {
        self.with(|p| {
            let bodies: Vec<&Body> = match &args.part {
                Some(id) => vec![
                    p.bodies
                        .iter()
                        .find(|body| &body.id == id)
                        .ok_or_else(|| format!("Unknown part: {id}"))?,
                ],
                None => p.bodies.iter().collect(),
            };
            let meshes: Vec<&faber_ports::Mesh> = bodies.iter().map(|body| &body.mesh).collect();
            let report =
                faber_core::overhang::analyse(&meshes, args.direction, args.critical_angle_deg)?;
            let regions: Vec<Value> = report
                .regions
                .iter()
                .map(|region| {
                    json!({"part":bodies[region.part].id,"centroid_mm":region.centroid_mm,
                        "area_mm2":region.area_mm2,"triangles":region.triangles})
                })
                .collect();
            Ok(
                json!({"schema":1,"revision":p.data.revision,"direction":args.direction,
                "critical_angle_deg":args.critical_angle_deg,"total_area_mm2":report.total_area_mm2,
                "support_area_mm2":report.support_area_mm2,"bed_area_mm2":report.bed_area_mm2,
                "flattest_overhang_deg":report.flattest_overhang_deg,
                "regions_total":report.regions_total,"regions":regions}),
            )
        })
    }

    #[tool(
        description = "Write one file per part. Choose stl for meshes or step for exact solids. Units are millimetres. A refused export lists each finding."
    )]
    fn export(&self, Parameters(Checked(args)): Parameters<Checked<Export>>) -> CallToolResult {
        self.with(|p| {
            let files = faber_wiring::export_checked(
                &p.bodies,
                &p.data.contacts,
                Path::new(&args.directory),
                &args.format,
                args.force,
            )?;
            Ok(json!({"schema":1,"files":files}))
        })
    }

    #[tool(
        description = "Read the modelling guide. Without topic, list the topics and read the quick start. With topic, read that section."
    )]
    fn guide(&self, Parameters(Checked(args)): Parameters<Checked<GuideArgs>>) -> CallToolResult {
        result(match args.topic {
            None => Ok(json!({
                "schema":1,
                "topics":guide_topics(GUIDE),
                "quick_start":guide_section(GUIDE, "Quick start"),
            })),
            Some(topic) => match guide_section(GUIDE, &topic) {
                Some(text) => Ok(json!({"schema":1,"topic":topic,"text":text})),
                None => Err(schema_tool::unknown(
                    "topic",
                    &topic,
                    &guide_topics(GUIDE),
                    "Call guide with no topic to list the topics.",
                )),
            },
        })
    }

    #[tool(
        description = "Read the shape of a recipe. Use kind geometry with no op for one line for each op. Set op to read one op schema with its definitions."
    )]
    fn schema(
        &self,
        Parameters(Checked(args)): Parameters<Checked<schema_tool::SchemaArgs>>,
    ) -> CallToolResult {
        result(schema_tool::read(args))
    }

    #[tool(
        description = "Read metric coarse pitch, clearance hole, tap drill and head dimensions in millimetres. Does not create a purchased part or a thread. No open project is required."
    )]
    fn metric_fastener(
        &self,
        Parameters(Checked(args)): Parameters<Checked<MetricFastener>>,
    ) -> CallToolResult {
        let Some(size) = faber_std::metric(&args.size) else {
            return result(Err(format!("Unknown metric size: {}", args.size)));
        };
        let head = size.socket_head();
        result(Ok(json!({
            "schema":1, "name":size.name(), "nominal_mm":size.nominal(),
            "pitch_mm":size.pitch(), "tap_drill_mm":size.tap_drill(),
            "clearance_mm":{
                "fine":size.clearance(faber_std::Fit::Fine),
                "medium":size.clearance(faber_std::Fit::Medium),
                "coarse":size.clearance(faber_std::Fit::Coarse)
            },
            "socket_head_mm":{"diameter":head.dk,"height":head.k},
            "counterbore":size.counterbore().map(|v| json!({"diameter_mm":v.diameter,"depth_mm":v.depth})),
            "countersink":size.countersink().map(|v| json!({"diameter_mm":v.head_diameter,"angle_degrees":v.angle}))
        })))
    }

    #[tool(
        description = "Render the model without a window to a PNG file at path. Set part to frame only that part. Set view to iso, front, back, left, right, top or bottom. Set section to a saved section id or to an inline plane. Set inline to true to also send the image. Source geometry stays unchanged."
    )]
    fn render(&self, Parameters(Checked(args)): Parameters<Checked<Render>>) -> CallToolResult {
        let mut response=self.with(|p| {
            let (section, bodies) = render_input(p, &args)?;
            faber_wiring::render_view(&bodies,Path::new(&args.path),section,args.width,args.height,args.view)?;
            Ok(json!({"schema":1,"path":args.path,"width":args.width,"height":args.height,"view":args.view,"part":args.part,"revision":p.data.revision}))
        });
        if args.inline && response.is_error != Some(true) {
            match std::fs::read(&args.path) {
                Ok(bytes) => response.content.push(ContentBlock::image(
                    base64::prelude::BASE64_STANDARD.encode(bytes),
                    "image/png",
                )),
                Err(e) => return result(Err(e.to_string())),
            }
        }
        response
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Faber {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("faber", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
