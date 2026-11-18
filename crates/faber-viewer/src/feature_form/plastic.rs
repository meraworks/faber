use super::{FeatureForm, Geometry, Scalar, Value};
use eframe::egui;
use serde_json::json;

/// Reference planes before the edited feature, newest first.
pub(crate) fn plane_ids(part: &Geometry<Scalar>, edit: &str) -> Vec<String> {
    let Geometry::History { features, .. } = part else {
        return Vec::new();
    };
    let end = features
        .iter()
        .position(|f| f.id == edit)
        .unwrap_or(features.len());
    features[..end]
        .iter()
        .rev()
        .filter(|f| !f.suppressed && crate::app::is_plane(&f.geometry))
        .map(|f| f.id.clone())
        .collect()
}

pub(super) fn choice_field(op: Option<&str>, key: &str) -> bool {
    match op {
        Some("rib") => matches!(key, "plane" | "side"),
        Some("web" | "boss" | "snap_hook") => key == "plane",
        Some("lip" | "groove") => matches!(key, "loop" | "side"),
        _ => false,
    }
}

impl FeatureForm {
    fn plastic_option(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        key: &str,
        default: Value,
    ) -> Result<(), String> {
        let mut on = !self.value[key].is_null();
        if ui.checkbox(&mut on, label).changed() {
            let mut value = self.geometry_value()?;
            if on {
                value[key] = default;
            } else {
                let fields = value.as_object_mut().ok_or("Invalid feature fields.")?;
                fields.remove(key);
                if key == "hole_diameter" {
                    fields.remove("hole_depth");
                }
            }
            self.replace_value(value);
        }
        Ok(())
    }

    pub(super) fn plastic_options(&mut self, ui: &mut egui::Ui) -> Result<(), String> {
        let op = self.value["op"].as_str();
        let rib = op == Some("rib");
        let wall = rib || op == Some("web");
        let boss = op == Some("boss");
        let hook = op == Some("snap_hook");
        if matches!(op, Some("lip" | "groove")) {
            choice(ui, &mut self.value["loop"], "Loop", || ["outer", "inner"]);
            choice(ui, &mut self.value["side"], "Loop side", || {
                ["inside", "outside"]
            });
            return Ok(());
        }

        if !(wall || boss || hook) {
            return Ok(());
        }
        choice(ui, &mut self.value["plane"], "Reference plane", || {
            self.expected
                .parts
                .get(&self.part)
                .map(|p| plane_ids(&p.geometry, &self.id))
                .unwrap_or_default()
        });
        if rib {
            choice(ui, &mut self.value["side"], "Wall side", || {
                ["symmetric", "left", "right"]
            });
        }
        if wall {
            let change = ui
                .horizontal(|ui| {
                    let add = ui.button("Add line").clicked();
                    let remove = ui
                        .add_enabled(
                            self.value["lines"].as_array().is_some_and(|l| l.len() > 1),
                            egui::Button::new("Remove last line"),
                        )
                        .clicked();
                    (add, remove)
                })
                .inner;
            if change.0 || change.1 {
                let mut value = self.geometry_value()?;
                let lines = value["lines"].as_array_mut().ok_or("Set the wall lines.")?;
                if change.0 {
                    let start = lines.last().map(|l| l[1].clone()).unwrap_or(json!([0, 0]));
                    lines.push(json!([start, [10, 10]]));
                } else {
                    lines.pop();
                }
                self.replace_value(value);
            }
        }
        if wall || boss {
            self.plastic_option(ui, "Set draft", "draft_deg", json!(1))?;
        }
        if boss {
            self.plastic_option(ui, "Add bore", "hole_diameter", json!(3))?;
            if !self.value["hole_diameter"].is_null() {
                self.plastic_option(ui, "Set bore depth", "hole_depth", json!(6))?;
            }
            self.plastic_option(
                ui,
                "Add radial ribs",
                "ribs",
                json!({"count":4,"thickness":1.5,"length":4,"height":4}),
            )?;
            self.plastic_option(ui, "Fillet the base", "fillet", json!(0.5))?;
        }
        if hook {
            for (label, key, default) in [
                ("Set tip thickness", "tip_thickness", 1.5),
                ("Set lead angle", "lead_deg", 30.),
                ("Set return angle", "return_deg", 90.),
                ("Fillet the root", "root_fillet", 0.5),
            ] {
                self.plastic_option(ui, label, key, json!(default))?;
            }
        }
        Ok(())
    }
}

fn choice<S: AsRef<str>, I: IntoIterator<Item = S>>(
    ui: &mut egui::Ui,
    value: &mut Value,
    label: &str,
    options: impl FnOnce() -> I,
) {
    ui.horizontal(|ui| {
        ui.menu_button(label, |ui| {
            for option in options() {
                let name = option.as_ref();
                if ui
                    .selectable_label(value.as_str() == Some(name), name)
                    .clicked()
                {
                    *value = name.into();
                    ui.close();
                }
            }
        });
        ui.label(value.as_str().unwrap_or_default());
    });
}
