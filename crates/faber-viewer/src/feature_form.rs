mod faces;
mod plastic;
pub(crate) mod surfaces;
pub(crate) use faces::{sketch_ids, split_tools};
pub(crate) use plastic::plane_ids;

use faber_model::{Geometry, ProjectData, Scalar};
use serde_json::Value;

pub fn label(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/lines/") {
        let indices: Vec<_> = rest
            .split('/')
            .filter_map(|p| p.parse::<usize>().ok())
            .collect();
        if let [line, end, axis] = indices.as_slice()
            && *end < 2
            && *axis < 2
        {
            return format!(
                "Line {} · {} · {}",
                line + 1,
                ["start", "end"][*end],
                ["X", "Y"][*axis]
            );
        }
    }
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    parts
        .iter()
        .enumerate()
        .map(|(position, part)| {
            if let Ok(index) = part.parse::<usize>() {
                let parent = position.checked_sub(1).map(|i| parts[i]).unwrap_or("");
                if matches!(
                    parent,
                    "size"
                        | "at"
                        | "step"
                        | "translation"
                        | "rotation"
                        | "vector"
                        | "axis"
                        | "parallel"
                        | "direction"
                        | "normal"
                        | "centre"
                        | "origin"
                        | "position"
                        | "near_centre"
                        | "start"
                        | "end"
                        | "via"
                        | "control1"
                        | "control2"
                        | "about"
                        | "factor"
                ) && index < 3
                {
                    ["X", "Y", "Z"][index].into()
                } else {
                    format!("Point {}", index + 1)
                }
            } else {
                part.replace('_', " ")
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Unit of a number or a parameter expression at a recipe path.
pub fn field_unit(path: &str, numeric: bool) -> Option<&'static str> {
    if !numeric {
        return None;
    }
    let key = path
        .rsplit('/')
        .find(|k| !k.is_empty() && !k.bytes().all(|c| c.is_ascii_digit()))?;
    match key {
        "draft_deg" | "lead_deg" | "return_deg" | "angle" | "angle_deg" | "rotation"
        | "start_deg" | "end_deg" | "sweep_deg" => Some("°"),
        "area" => Some("mm²"),
        "count" | "expect" | "turns" | "factor" | "parallel" | "normal" | "direction"
        | "weights" | "knots" => Some("1"),
        "mass_g" => Some("g"),
        "axis" | "degree" | "sides" | "edge" | "k_factor" => Some("1"),
        _ => Some("mm"),
    }
}

pub fn field(
    ui: &mut eframe::egui::Ui,
    label: &str,
    value: &mut String,
    unit: Option<&str>,
    params: &std::collections::BTreeMap<String, f64>,
) {
    let label = ui.label(label);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(value).labelled_by(label.id);
            if let Some(unit) = unit.filter(|u| !u.is_empty()) {
                ui.label(unit);
            }
        });
        if let Some(unit) = unit
            && value.parse::<f64>().is_err()
            && let Ok(n) = faber_expr::evaluate(value, |id| {
                params.get(id).copied().ok_or_else(|| id.to_owned())
            })
        {
            ui.small(format!("= {n} {unit}").trim_end());
        }
    });
}

pub struct FeatureForm {
    pub(crate) surface_pick: Option<(String, String)>,
    pub(crate) solid_edge_path: Option<String>,
    pub help: &'static str,
    pub create: bool,
    pub expected: ProjectData,
    pub part: String,
    pub id: String,
    value: Value,
    pub fields: Vec<(String, String, bool)>,
    pub flags: Vec<(String, bool)>,
}

impl FeatureForm {
    pub fn new(
        expected: ProjectData,
        part: String,
        id: String,
        geometry: &Geometry<Scalar>,
    ) -> Result<Self, String> {
        let value = serde_json::to_value(geometry).map_err(|e| e.to_string())?;
        let mut fields = vec![];
        let mut flags = vec![];
        collect_fields(&value, &mut fields, &mut flags);
        Ok(Self {
            help: match geometry {
                Geometry::PlaneOnFace { .. } => {
                    "Select exactly one planar face. The plane origin is its area centre. Positive offset follows the outward normal. The current solid remains the part output."
                }
                Geometry::Plane { .. } => {
                    "Define a reference plane with translation and rotation in part coordinates. The current solid remains the part output."
                }
                Geometry::Axis { .. } => {
                    "Define an origin and non-zero direction in part coordinates. This reference can be used as a revolve axis. The current solid remains the part output."
                }
                Geometry::Point { .. } => {
                    "Define a reference point in part coordinates. The current solid remains the part output."
                }
                Geometry::PlaneAtAngle { .. } => {
                    "The axis must lie in the reference plane. Set the angle in degrees about the axis. Base planes are XY, XZ and YZ. The current solid remains the part output."
                }
                Geometry::Midplane { .. } => {
                    "Set two parallel reference planes. The result sits halfway between them. Non-parallel planes are refused. The current solid remains the part output."
                }
                Geometry::PlaneThroughPoints { .. } => {
                    "Set three points as coordinates or point feature ids. The points must not lie on one line. The current solid remains the part output."
                }
                Geometry::AxisFromCylinder { .. } => {
                    "Select exactly one cylinder face. The axis is the cylinder axis. The current solid remains the part output."
                }
                Geometry::AxisFromPlanes { .. } => {
                    "Set two reference planes that are not parallel. The axis is their line of intersection. The current solid remains the part output."
                }
                Geometry::PointAtCentre { .. } => {
                    "Select exactly one full circular edge. The point is its centre. The current solid remains the part output."
                }
                Geometry::SheetMetal { .. } => {
                    "Set the stock thickness, inside bend radius and K factor. Flange length starts at the end of the bend. Straight edge numbers start at one."
                }
                Geometry::Split { .. } => {
                    "Origin and normal define the cut plane in part coordinates. Positive side follows the normal. Keep both sides or one side. The plane must cut through the solid."
                }
                Geometry::Thread { .. } => {
                    "Select one cylinder by its radius and side. Set the metric size, axial length and start offset in millimetres. Select internal for a bore."
                }
                Geometry::Hole { .. } => {
                    "Translation sets the entry point in part coordinates. The cut follows local +Z after rotation. Set diameter and depth in millimetres; countersink angle is the included angle in degrees."
                }
                Geometry::RemoveFaces { .. } => {
                    "Remove the selected faces. The adjacent faces must close the solid."
                }
                Geometry::MoveFaces { .. } => {
                    "Positive distance adds material. Negative distance removes material."
                }
                Geometry::SplitFace { .. } => {
                    "Choose a reference plane or a profile sketch that crosses the selected face."
                }
                Geometry::Emboss { .. } => {
                    "Choose a closed profile or text sketch. Set the depth in mm. On a cylinder, the sketch X coordinate follows the circumference."
                }
                Geometry::Thicken { .. } => {
                    "Build a solid from the selected face. Positive thickness follows its normal; negative thickness uses the opposite direction. This becomes the part output."
                }
                Geometry::Draft { .. } => {
                    "Select faces by normal. Origin and normal define the neutral plane in part coordinates. Positive angle removes material on the direction side. Angles use degrees."
                }
                Geometry::OffsetSketch { .. } => {
                    "Select a source sketch. A negative distance shrinks the outer boundary."
                }
                Geometry::Offset { .. } => {
                    "Positive distance expands the solid with round joins. Negative distance contracts it. Zero distance is refused."
                }
                Geometry::Fillet { .. } | Geometry::Chamfer { .. } => {
                    "Select straight edges by direction in part coordinates. Set the expected edge count. The default selects four edges parallel to Z. Dimensions use millimetres."
                }
                Geometry::Shell { .. } => {
                    "Remove the faces that match the normal in part coordinates. Thickness extends inwards. The default opens the top face."
                }
                Geometry::LinearPattern { .. } => {
                    "Count includes the source. Step uses the part axes."
                }
                Geometry::CircularPattern { .. } => {
                    "Count includes the source. Angle is the gap between copies. An angle of 360 degrees spaces the copies equally around the circle. Origin and axis use part coordinates."
                }
                Geometry::Mirror { .. } => {
                    "Origin is a point on the mirror plane. Normal gives its direction in part coordinates."
                }
                Geometry::Scale { .. } => {
                    "Scale the solid about a point in part coordinates. One factor scales all axes. Set Factor per axis to give x, y and z their own factor. Each factor must be greater than zero."
                }
                Geometry::Pipe { .. } => {
                    "Sweep a round section along a curve or helix feature of this part. Set the diameter in millimetres. Set a wall to make a tube."
                }
                Geometry::PathPattern { .. } => {
                    "Copy the source along a curve or helix feature. Count includes the source. Without spacing, the copies span the whole path. Orient turns each copy to the path."
                }
                Geometry::Rib { .. } => {
                    "Set each line in reference plane coordinates. Its ends must touch the body. The rib grows in the plane."
                }
                Geometry::Web { .. } => {
                    "Set each line in reference plane coordinates. The web grows to the body along the plane normal."
                }
                Geometry::Boss { .. } => {
                    "Set the centre on the reference plane. Height follows the plane normal. Set a bore, ribs or a base fillet as needed."
                }
                Geometry::SnapHook { .. } => {
                    "Set the root on the reference plane. The beam grows along the plane normal. The angle turns the hook about this normal."
                }
                Geometry::Lip { .. } => {
                    "Pick one planar face. Choose its outer or inner loop and the side to add material."
                }
                Geometry::Groove { .. } => {
                    "Pick one planar face. Clearance adds space at both sides and at the bottom of the groove."
                }
                _ => "",
            },
            create: false,
            surface_pick: None,
            solid_edge_path: None,
            expected,
            part,
            id,
            value,
            fields,
            flags,
        })
    }

    pub fn split_options(&mut self, ui: &mut eframe::egui::Ui) {
        if !matches!(self.value["op"].as_str(), Some("split" | "split_body")) {
            return;
        }
        let mut selected = self.value["keep"].as_str().unwrap_or("both").to_owned();
        ui.horizontal(|ui| {
            for (value, title) in [
                ("both", "Both sides"),
                ("positive", "Positive side"),
                ("negative", "Negative side"),
            ] {
                ui.selectable_value(&mut selected, value.to_owned(), title);
            }
        });
        self.value["keep"] = Value::String(selected);
    }

    pub fn hole_options(&mut self, ui: &mut eframe::egui::Ui) -> Result<bool, String> {
        if self.value["op"] != "hole" {
            return Ok(false);
        }
        let mut preset = None;
        ui.menu_button("Metric preset", |ui| {
            for size in faber_std::sizes() {
                ui.horizontal(|ui| {
                    if ui.button(format!("{} clearance", size.name())).clicked() {
                        preset = Some((size.name(), false));
                        ui.close();
                    }
                    if ui.button(format!("{} tap drill", size.name())).clicked() {
                        preset = Some((size.name(), true));
                        ui.close();
                    }
                });
            }
        });
        if let Some((size, tap)) = preset {
            self.apply_metric(size, tap)?;
        }
        let current = self.value["head"]["kind"].as_str().unwrap_or("plain");
        let mut selected = current;
        ui.horizontal(|ui| {
            for (kind, title) in [
                ("plain", "Plain"),
                ("counterbore", "Counterbore"),
                ("countersink", "Countersink"),
            ] {
                ui.selectable_value(&mut selected, kind, title);
            }
        });
        if selected != current {
            let head = match selected {
                "counterbore" => serde_json::json!({"kind":"counterbore", "diameter":8, "depth":3}),
                "countersink" => {
                    serde_json::json!({"kind":"countersink", "diameter":8, "angle":90})
                }
                _ => Value::Null,
            };
            let mut value = serde_json::to_value(self.geometry()?).map_err(|e| e.to_string())?;
            value["head"] = head;
            self.fields.clear();
            self.flags.clear();
            collect_fields(&value, &mut self.fields, &mut self.flags);
            self.value = value;
        }
        Ok(preset.is_some())
    }

    fn apply_metric(&mut self, name: &str, tap: bool) -> Result<(), String> {
        let size = faber_std::metric(name).ok_or("Unknown metric size")?;
        let mut value = serde_json::to_value(self.geometry()?).map_err(|e| e.to_string())?;
        if value["op"] != "hole" {
            return Err("Metric presets require a hole".into());
        }
        match value["head"]["kind"].as_str() {
            Some("counterbore") => {
                let head = size
                    .counterbore()
                    .ok_or("No counterbore dimensions for this size")?;
                value["head"] = serde_json::json!({"kind":"counterbore","diameter":head.diameter,"depth":head.depth});
            }
            Some("countersink") => {
                let head = size
                    .countersink()
                    .ok_or("No countersink dimensions for this size")?;
                value["head"] = serde_json::json!({"kind":"countersink","diameter":head.head_diameter,"angle":head.angle});
            }
            _ => {}
        }
        value["diameter"] = serde_json::json!(if tap {
            size.tap_drill()
        } else {
            size.clearance(faber_std::Fit::Medium)
        });
        self.fields.clear();
        self.flags.clear();
        collect_fields(&value, &mut self.fields, &mut self.flags);
        self.value = value;
        Ok(())
    }

    /// Path drop-down for `pipe` and `path_pattern`. An inline curve has no drop-down.
    pub fn path_options(&mut self, ui: &mut eframe::egui::Ui) {
        if !is_path_recipe(&self.value) || !self.value["path"].is_string() {
            return;
        }
        let current = self.value["path"].as_str().unwrap_or_default().to_owned();
        let ids = self
            .expected
            .parts
            .get(&self.part)
            .map(|part| path_ids(&part.geometry))
            .unwrap_or_default();
        let mut selected = current.clone();
        ui.horizontal(|ui| {
            ui.label("Path");
            eframe::egui::ComboBox::from_id_salt("feature_path")
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    for id in &ids {
                        ui.selectable_value(&mut selected, id.clone(), id.as_str());
                    }
                });
        });
        if ids.is_empty() {
            ui.label("This part has no curve or helix feature.");
        }
        if selected != current {
            self.value["path"] = Value::String(selected);
        }
    }

    /// Optional fields. Scale can use one factor per axis. Pipe and path pattern can omit a field.
    pub fn option_fields(&mut self, ui: &mut eframe::egui::Ui) -> Result<(), String> {
        self.face_options(ui)?;
        self.surface_options(ui)?;
        self.plastic_options(ui)?;
        match self.value["op"].as_str().unwrap_or_default() {
            "scale" => {
                let mut per_axis = self.value["factor"].is_array();
                if ui.checkbox(&mut per_axis, "Factor per axis").changed() {
                    let mut value = self.geometry_value()?;
                    let factor = value["factor"]
                        .as_array()
                        .and_then(|axes| axes.first().cloned())
                        .unwrap_or_else(|| value["factor"].clone());
                    value["factor"] = if per_axis {
                        serde_json::json!([factor.clone(), factor.clone(), factor])
                    } else {
                        factor
                    };
                    self.replace_value(value);
                }
            }
            "pipe" => {
                let mut hollow = self.value.get("wall").is_some();
                if ui.checkbox(&mut hollow, "Hollow tube").changed() {
                    let mut value = self.geometry_value()?;
                    toggle_field(&mut value, "wall", hollow, 1.);
                    self.replace_value(value);
                }
            }
            "path_pattern" => {
                let mut spaced = self.value.get("spacing").is_some();
                if ui.checkbox(&mut spaced, "Set spacing").changed() {
                    let mut value = self.geometry_value()?;
                    toggle_field(&mut value, "spacing", spaced, 10.);
                    self.replace_value(value);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn geometry_value(&self) -> Result<Value, String> {
        serde_json::to_value(self.geometry()?).map_err(|e| e.to_string())
    }

    fn replace_value(&mut self, value: Value) {
        self.fields.clear();
        self.flags.clear();
        collect_fields(&value, &mut self.fields, &mut self.flags);
        self.value = value;
    }

    pub(crate) fn requires_planar_pick(&self) -> bool {
        matches!(
            self.value["op"].as_str(),
            Some("plane_on_face" | "lip" | "groove")
        )
    }

    pub(crate) fn pick_field(&self) -> Option<(&'static str, &'static str)> {
        if self.solid_edge_path.is_some() {
            return Some(("Pick edge", "edges"));
        }
        if self.value["op"] == "thicken" && self.value["face"].is_null() {
            return None;
        }
        match self.value["op"].as_str()? {
            "fillet" | "chamfer" => Some(("Edges", "edges")),
            "remove_faces" | "move_faces" | "shell" | "draft" => Some(("Faces", "faces")),
            "replace_face" | "split_face" | "emboss" | "plane_on_face" | "thicken" | "lip"
            | "groove" => Some(("Faces", "face")),
            "hole" => Some(("Face", "placement")),
            _ => None,
        }
    }

    pub(crate) fn set_picked(&mut self, key: &str, field: Value) -> Result<(), String> {
        if let Some(path) = self.solid_edge_path.take() {
            return self.apply_surface_edge(&path, field);
        }
        let mut value = self.geometry_value()?;
        let displayed = match &self
            .expected
            .parts
            .get(&self.part)
            .ok_or("Select a part")?
            .geometry
        {
            Geometry::History { output, .. } => output.as_str(),
            _ => "source",
        };
        let source = value.get("body").or_else(|| value.get("source"));
        if source.and_then(Value::as_str) != Some(displayed) {
            return Err("The displayed solid differs from the feature source. Set the selector fields manually.".into());
        }
        value[key] = field;
        self.replace_value(value);
        Ok(())
    }

    pub fn geometry(&self) -> Result<Geometry<Scalar>, String> {
        let mut value = self.value.clone();
        for (path, enabled) in &self.flags {
            *value.pointer_mut(path).ok_or("Feature field is missing")? = Value::Bool(*enabled);
        }
        for (path, text, numeric) in &self.fields {
            *value.pointer_mut(path).ok_or("Feature field is missing")? = if *numeric {
                serde_json::from_str::<serde_json::Number>(text)
                    .map(Value::Number)
                    .unwrap_or_else(|_| Value::String(text.clone()))
            } else {
                Value::String(text.clone())
            };
        }
        serde_json::from_value(value).map_err(|e| format!("Invalid feature value: {e}"))
    }
}

/// Feature ids of the curve and helix features of a part. Pipe and path pattern use them as paths.
pub fn path_ids(part: &Geometry<Scalar>) -> Vec<String> {
    match part {
        Geometry::History { features, .. } => features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.geometry,
                    Geometry::Curve { .. } | Geometry::Helix { .. }
                )
            })
            .map(|feature| feature.id.clone())
            .collect(),
        _ => Vec::new(),
    }
}

fn is_path_recipe(value: &Value) -> bool {
    matches!(value["op"].as_str(), Some("pipe" | "path_pattern"))
}

/// Adds the field with a default when on. Removes it when off.
fn toggle_field(value: &mut Value, key: &str, on: bool, default: f64) {
    if on {
        if value.get(key).is_none() {
            value[key] = serde_json::json!(default);
        }
    } else if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}

fn collect_fields(
    value: &Value,
    fields: &mut Vec<(String, String, bool)>,
    flags: &mut Vec<(String, bool)>,
) {
    collect(value, "", fields, flags);
    // Resolve the type of string fields without evaluating parameter expressions.
    let numeric = serde_json::from_value::<Geometry<Scalar>>(value.clone())
        .ok()
        .and_then(|geometry| geometry.map_numbers(&mut |_| Ok(1.0)).ok())
        .and_then(|geometry| serde_json::to_value(geometry).ok());
    if let Some(numeric) = numeric {
        for (path, _, number) in fields {
            *number |= numeric.pointer(path).is_some_and(Value::is_number);
        }
    }
}

fn collect(
    value: &Value,
    path: &str,
    fields: &mut Vec<(String, String, bool)>,
    flags: &mut Vec<(String, bool)>,
) {
    match value {
        Value::Object(values) => {
            // The path of pipe and path pattern has a drop-down, not a text field.
            let feature_path =
                is_path_recipe(value) && values.get("path").is_some_and(Value::is_string);
            let op = value["op"].as_str();
            for (key, value) in values {
                if key != "op"
                    && (key != "keep" || op == Some("trim"))
                    && !surfaces::choice(op, key)
                    && !(op == Some("surface_sweep") && key == "path" && value.is_string())
                    && !(path.starts_with("/boundary/")
                        && matches!(key.as_str(), "kind" | "feature"))
                    && !plastic::choice_field(op, key)
                    && !faces::choice_field(op, key)
                    && !(path == "/head" && key == "kind")
                    && !(feature_path && key == "path")
                {
                    collect(value, &format!("{path}/{key}"), fields, flags);
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect(value, &format!("{path}/{index}"), fields, flags);
            }
        }
        Value::Number(number) => fields.push((path.into(), number.to_string(), true)),
        Value::Bool(enabled) => flags.push((path.into(), *enabled)),
        Value::String(text) => fields.push((path.into(), text.clone(), false)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plastic_feature_angles_show_degrees() {
        for path in ["/draft_deg", "/lead_deg", "/return_deg"] {
            assert_eq!(field_unit(path, true), Some("°"), "{path}");
        }
        assert_eq!(field_unit("/ribs/count", true), Some("1"));
        assert_eq!(field_unit("/rule/k_factor", true), Some("1"));
        assert_eq!(field_unit("/flanges/0/edge", true), Some("1"));
        assert_eq!(field_unit("/flanges/0/inset_start", true), Some("mm"));
        assert_eq!(field_unit("/lines/0/1/0", true), Some("mm"));
    }

    #[test]
    fn plastic_feature_picks_one_planar_face_on_the_displayed_body() {
        let data: ProjectData = serde_json::from_value(serde_json::json!({
            "schema":2,"revision":0,"name":"Tray","params":{},
            "parts":{"tray":{"name":"Tray","colour":[80,90,120],
                "geometry":{"op":"history","output":"base",
                    "features":[{"id":"base","geometry":{"op":"box","size":[40,40,5]}}]}}}
        }))
        .unwrap();
        let selected = serde_json::json!({"normal":[0,0,1],"near_centre":[20,20,5],"tolerance":0.001,"expect":1});
        for op in ["lip", "groove"] {
            let mut recipe = serde_json::json!({"op":op,"body":"base","face":{"normal":[0,0,1],"tolerance":0.001,"expect":1},
                "loop":"outer","side":"inside","width":1});
            if op == "lip" {
                recipe["height"] = 2.into();
            } else {
                recipe["depth"] = 2.into();
                recipe["clearance"] = 0.15.into();
            }
            let geometry = serde_json::from_value(recipe).unwrap();
            let mut form =
                FeatureForm::new(data.clone(), "tray".into(), "rim".into(), &geometry).unwrap();
            assert_eq!(form.pick_field(), Some(("Faces", "face")));
            form.set_picked("face", selected.clone()).unwrap();
            let saved = serde_json::to_value(form.geometry().unwrap()).unwrap();
            assert_eq!(saved["face"]["near_centre"][2], 5.);
        }
    }

    #[test]
    fn metric_hole_presets_preserve_depth_and_placement() {
        let geometry: Geometry<Scalar> = serde_json::from_value(serde_json::json!({
            "op":"hole", "source":"base", "diameter":4, "depth":"wall+2",
            "placement":{"translation":[3,5,7],"rotation":[180,0,0]},
            "head":{"kind":"counterbore","diameter":8,"depth":4}
        }))
        .unwrap();
        let data = serde_json::from_value(
            serde_json::json!({"schema":1,"revision":0,"name":"Form","parts":{},"params":{}}),
        )
        .unwrap();
        let mut form = FeatureForm::new(data, "body".into(), "hole".into(), &geometry).unwrap();
        form.apply_metric("M3", false).unwrap();
        let v = serde_json::to_value(form.geometry().unwrap()).unwrap();
        assert_eq!(v["diameter"], 3.4);
        assert_eq!(v["depth"], "wall+2");
        assert_eq!(
            v["placement"]["translation"],
            serde_json::json!([3.0, 5.0, 7.0])
        );
        assert_eq!(v["head"]["depth"], 3.2);
        let before = form.geometry().unwrap();
        assert!(form.apply_metric("M1.6", false).is_err());
        assert_eq!(form.geometry().unwrap(), before);
        form.apply_metric("M3", true).unwrap();
        let v = serde_json::to_value(form.geometry().unwrap()).unwrap();
        assert_eq!(v["diameter"], 2.5);
        assert_eq!(v["head"]["kind"], "counterbore");
    }

    #[test]
    fn form_preserves_recipe_and_accepts_parameter_expressions() {
        let geometry: Geometry<Scalar> =
            serde_json::from_value(serde_json::json!({"op":"box","size":[10,20,30]})).unwrap();
        let data: ProjectData = serde_json::from_value(
            serde_json::json!({"schema":1,"revision":0,"name":"Form","parts":{},"params":{}}),
        )
        .unwrap();
        let mut form = FeatureForm::new(data, "body".into(), "base".into(), &geometry).unwrap();
        assert_eq!(form.geometry().unwrap(), geometry);
        form.fields
            .iter_mut()
            .find(|(path, _, _)| path == "/size/2")
            .unwrap()
            .1 = "height * 2".into();
        assert_eq!(
            serde_json::to_value(form.geometry().unwrap()).unwrap()["size"][2],
            "height * 2"
        );
    }
}
