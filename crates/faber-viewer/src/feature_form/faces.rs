use super::{FeatureForm, Geometry, Scalar};
use eframe::egui;
use faber_model::SplitTool;
use serde_json::json;

fn prior<'a>(
    geometry: &'a Geometry<Scalar>,
    edit: &str,
) -> impl Iterator<Item = &'a faber_model::Feature<Scalar>> {
    let features = match geometry {
        Geometry::History { features, .. } => features.as_slice(),
        _ => &[],
    };
    let end = features
        .iter()
        .position(|f| f.id == edit)
        .unwrap_or(features.len());
    features[..end].iter().rev().filter(|f| !f.suppressed)
}
pub(crate) fn sketch_ids(geometry: &Geometry<Scalar>, edit: &str) -> Vec<String> {
    prior(geometry, edit)
        .filter(|f| {
            matches!(
                &f.geometry,
                Geometry::Sketch { .. } | Geometry::SketchOnPlane { .. }
            )
        })
        .map(|f| f.id.clone())
        .collect()
}
pub(crate) fn split_tools(geometry: &Geometry<Scalar>, edit: &str) -> Vec<(SplitTool, String)> {
    let planes = prior(geometry, edit)
        .filter(|f| crate::app::is_plane(&f.geometry))
        .map(|f| {
            (
                SplitTool::Plane {
                    plane: f.id.clone(),
                },
                format!("Plane · {}", f.id),
            )
        });
    planes
        .chain(sketch_ids(geometry, edit).into_iter().map(|id| {
            (
                SplitTool::Sketch { sketch: id.clone() },
                format!("Sketch · {id}"),
            )
        }))
        .collect()
}
pub(super) fn choice_field(op: Option<&str>, key: &str) -> bool {
    matches!(
        (op, key),
        (Some("split_face"), "tool") | (Some("emboss"), "mode" | "sketch")
    )
}

impl FeatureForm {
    pub(super) fn face_options(&mut self, ui: &mut egui::Ui) -> Result<(), String> {
        let part = &self.expected.parts[&self.part];
        match self.value["op"].as_str() {
            Some("split_face") => {
                let options = split_tools(&part.geometry, &self.id);
                let current: SplitTool = serde_json::from_value(self.value["tool"].clone())
                    .map_err(|e| e.to_string())?;
                let selected = options
                    .iter()
                    .find(|(t, _)| *t == current)
                    .map_or("Choose a tool", |(_, label)| label);
                egui::ComboBox::from_label("Splitting tool")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (tool, label) in &options {
                            if ui.selectable_label(*tool == current, label).clicked() {
                                self.value["tool"] =
                                    serde_json::to_value(tool).expect("Split tool fields");
                            }
                        }
                    });
            }
            Some("emboss") => {
                egui::ComboBox::from_label("Profile sketch")
                    .selected_text(self.value["sketch"].as_str().unwrap_or("Choose a sketch"))
                    .show_ui(ui, |ui| {
                        for id in sketch_ids(&part.geometry, &self.id) {
                            if ui
                                .selectable_label(
                                    self.value["sketch"].as_str() == Some(id.as_str()),
                                    &id,
                                )
                                .clicked()
                            {
                                self.value["sketch"] = id.into();
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    for (name, mode) in [("Raise", "raise"), ("Engrave", "engrave")] {
                        if ui
                            .selectable_label(self.value["mode"] == mode, name)
                            .clicked()
                        {
                            self.value["mode"] = mode.into();
                        }
                    }
                });
                let mut located = self.value["at"].is_array();
                if ui
                    .checkbox(&mut located, "Set cylinder arc and height")
                    .changed()
                {
                    let mut value = self.geometry_value()?;
                    if located {
                        value["at"] = json!([0., 0.]);
                    } else {
                        value.as_object_mut().unwrap().remove("at");
                    }
                    self.replace_value(value);
                }
            }
            _ => (),
        }
        Ok(())
    }

    pub(crate) fn picks_face_list(&self) -> bool {
        matches!(
            self.value["op"].as_str(),
            Some("remove_faces" | "move_faces")
        )
    }
}
