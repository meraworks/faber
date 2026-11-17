use faber_model::{Geometry, ProjectData, Scalar};
use serde_json::Value;

pub fn label(path: &str) -> String {
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

pub struct FeatureForm {
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
        collect(&value, "", &mut fields, &mut flags);
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
                Geometry::Split { .. } => {
                    "Origin and normal define the cut plane in part coordinates. Positive side follows the normal. Keep both sides or one side. The plane must cut through the solid."
                }
                Geometry::Hole { .. } => {
                    "Translation sets the entry point in part coordinates. The cut follows local +Z after rotation. Set diameter and depth in millimetres; countersink angle is the included angle in degrees."
                }
                Geometry::Thicken { .. } => {
                    "Build a solid from the selected face. Positive thickness follows its normal; negative thickness uses the opposite direction. This becomes the part output."
                }
                Geometry::Draft { .. } => {
                    "Select faces by normal. Origin and normal define the neutral plane in part coordinates. Positive angle removes material on the direction side. Angles use degrees."
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
                _ => "",
            },
            create: false,
            expected,
            part,
            id,
            value,
            fields,
            flags,
        })
    }

    pub fn split_options(&mut self, ui: &mut eframe::egui::Ui) {
        if self.value["op"] != "split" {
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

    pub fn hole_options(&mut self, ui: &mut eframe::egui::Ui) -> Result<(), String> {
        if self.value["op"] != "hole" {
            return Ok(());
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
            collect(&value, "", &mut self.fields, &mut self.flags);
            self.value = value;
        }
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

fn collect(
    value: &Value,
    path: &str,
    fields: &mut Vec<(String, String, bool)>,
    flags: &mut Vec<(String, bool)>,
) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if key != "op" && key != "keep" && !(path == "/head" && key == "kind") {
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
