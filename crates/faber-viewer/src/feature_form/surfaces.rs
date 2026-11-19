use super::{FeatureForm, Geometry, Scalar};
use eframe::egui;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceTool {
    Extrude,
    Revolve,
    Loft,
    Sweep,
    Patch,
    Trim,
    Extend,
    Stitch,
    Split,
    Replace,
}
impl SurfaceTool {
    pub(crate) const ALL: [Self; 10] = [
        Self::Extrude,
        Self::Revolve,
        Self::Loft,
        Self::Sweep,
        Self::Patch,
        Self::Trim,
        Self::Extend,
        Self::Stitch,
        Self::Split,
        Self::Replace,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Extrude => "Surface extrude",
            Self::Revolve => "Surface revolve",
            Self::Loft => "Surface loft",
            Self::Sweep => "Surface sweep",
            Self::Patch => "Patch",
            Self::Trim => "Trim surface",
            Self::Extend => "Extend surface",
            Self::Stitch => "Stitch",
            Self::Split => "Split with surface",
            Self::Replace => "Replace face",
        }
    }
    pub(crate) fn op(self) -> &'static str {
        match self {
            Self::Extrude => "surface_extrude",
            Self::Revolve => "surface_revolve",
            Self::Loft => "surface_loft",
            Self::Sweep => "surface_sweep",
            Self::Patch => "patch",
            Self::Trim => "trim",
            Self::Extend => "extend",
            Self::Stitch => "stitch",
            Self::Split => "split_body",
            Self::Replace => "replace_face",
        }
    }
    pub(crate) fn geometry(
        self,
        part: &Geometry<Scalar>,
        body: String,
    ) -> Result<Geometry<Scalar>, String> {
        let ids = Inputs::new(part, "");
        let first = |set: &[String], name: &str| {
            set.first()
                .cloned()
                .ok_or_else(|| format!("Add {name} to the part first."))
        };
        let profile = || first(&ids.profiles, "a sketch or curve");
        let surf = || first(&ids.surfaces, "a surface");
        let mut v = match self {
            Self::Extrude => json!({"profile":profile()?,"distance":10}),
            Self::Revolve => {
                json!({"profile":profile()?,"axis":first(&ids.axes,"a reference axis")?,"angle_deg":360})
            }
            Self::Loft => {
                if ids.profiles.len() < 2 {
                    return Err("Add at least two profiles to loft.".into());
                }
                json!({"sections":&ids.profiles[..2],"ruled":false})
            }
            Self::Sweep => json!({"profile":profile()?,"path":first(&ids.paths,"a path")?}),
            Self::Patch => {
                let boundary = ids
                    .profiles
                    .first()
                    .map(|s| json!({"kind":"curve","feature":s}))
                    .unwrap_or_else(|| json!({"kind":"edge","feature":body,"edge":edge()}));
                json!({"boundary":[boundary],"tangent_to":[]})
            }
            Self::Trim => {
                let s = surf()?;
                let t = ids
                    .tools
                    .iter()
                    .find(|t| **t != s)
                    .ok_or("Add a second surface, plane or sketch to trim the surface.")?;
                json!({"surface":s,"tool":t,"keep":{"near":[0,0,0]}})
            }
            Self::Extend => json!({"surface":surf()?,"edge":edge(),"distance":5}),
            Self::Stitch => {
                if ids.surfaces.is_empty() {
                    return Err("Add a surface to stitch first.".into());
                }
                json!({"surfaces":ids.surfaces,"as_solid":false})
            }
            Self::Split => json!({"body":body,"tool":surf()?,"keep":"both"}),
            Self::Replace => {
                json!({"body":body,"face":{"normal":[0,0,1],"tolerance":0.00001,"expect":1},"surface":surf()?})
            }
        };
        v["op"] = self.op().into();
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
}
fn edge() -> Value {
    json!({"near_centre":[0,0,0],"expect":1,"tolerance":0.001})
}
pub(crate) fn surface(
    features: &[faber_model::Feature<Scalar>],
    feature: &faber_model::Feature<Scalar>,
) -> bool {
    use faber_core::surface_feature::{creates_surface, is_surface_output};
    !feature.suppressed
        && (creates_surface(&feature.geometry)
            || matches!(
                feature.geometry,
                Geometry::Transform { .. } | Geometry::Scale { .. } | Geometry::PlaceAtPoint { .. }
            ) && is_surface_output(features, &feature.id))
}
#[derive(Default)]
struct Inputs {
    profiles: Vec<String>,
    paths: Vec<String>,
    surfaces: Vec<String>,
    axes: Vec<String>,
    tools: Vec<String>,
    solids: Vec<String>,
}
impl Inputs {
    fn new(g: &Geometry<Scalar>, before: &str) -> Self {
        let mut v = Self::default();
        let Geometry::History { features, .. } = g else {
            v.solids.push("source".into());
            return v;
        };
        for f in features.iter().take_while(|f| f.id != before) {
            let g = if f.suppressed
                || matches!(
                    f.geometry,
                    Geometry::Transform { .. }
                        | Geometry::Scale { .. }
                        | Geometry::PlaceAtPoint { .. }
                ) {
                let Some(source) = faber_core::surface_feature::source_geometry(features, &f.id)
                else {
                    continue;
                };
                source
            } else {
                &f.geometry
            };
            if matches!(
                g,
                Geometry::Sketch { .. }
                    | Geometry::SolvedSketch { .. }
                    | Geometry::SketchOnPlane { .. }
                    | Geometry::OffsetSketch { .. }
            ) {
                v.profiles.push(f.id.clone());
                v.tools.push(f.id.clone());
            } else if matches!(g, Geometry::Curve { .. } | Geometry::Helix { .. }) {
                v.profiles.push(f.id.clone());
                v.paths.push(f.id.clone());
            } else if faber_core::surface_feature::creates_surface(g) {
                v.surfaces.push(f.id.clone());
                v.tools.push(f.id.clone());
            } else if crate::app::is_plane(g) {
                v.tools.push(f.id.clone());
            } else if crate::app::is_axis(g) {
                v.axes.push(f.id.clone());
            } else if !matches!(g, Geometry::Point { .. } | Geometry::PointAtCentre { .. }) {
                v.solids.push(f.id.clone());
            }
        }
        v
    }
}
pub(super) fn choice(op: Option<&str>, key: &str) -> bool {
    op.is_some_and(|s| SurfaceTool::ALL.iter().any(|t| t.op() == s))
        && matches!(
            key,
            "profile"
                | "axis"
                | "sections"
                | "surfaces"
                | "surface"
                | "body"
                | "tool"
                | "tangent_to"
        )
}
fn select(ui: &mut egui::Ui, label: &str, value: &mut Value, ids: &[String]) -> bool {
    let old = value.clone();
    egui::ComboBox::from_label(label)
        .selected_text(value.as_str().unwrap_or("Choose feature"))
        .truncate()
        .show_ui(ui, |ui| {
            for id in ids {
                if ui
                    .selectable_label(value.as_str() == Some(id), id)
                    .clicked()
                {
                    *value = json!(id);
                }
            }
        });
    old != *value
}
fn list(
    ui: &mut egui::Ui,
    key: &str,
    label: &str,
    items: &mut Vec<Value>,
    choices: &[String],
    boundary: bool,
    edges: &[String],
) -> bool {
    let mut remove = None;
    let mut swap = None;
    let mut changed = false;
    for (i, item) in items.iter_mut().enumerate() {
        ui.push_id((key, i), |ui| {
            if boundary {
                let is_edge = item["kind"] == "edge";
                let mut next = is_edge;
                if ui.checkbox(&mut next, "Use selected edge").changed() {
                    let ids = if next { edges } else { choices };
                    let id = ids.first().cloned().unwrap_or_default();
                    *item = if next {
                        json!({"kind":"edge","feature":id,"edge":edge()})
                    } else {
                        json!({"kind":"curve","feature":id})
                    };
                    changed = true;
                }
                let ids = if next { edges } else { choices };
                changed |= select(ui, &format!("{label} {}", i + 1), &mut item["feature"], ids);
            } else {
                changed |= select(ui, &format!("{label} {}", i + 1), item, choices);
            }
            ui.horizontal(|ui| {
                if i > 0 && ui.small_button("Move up").clicked() {
                    swap = Some(i);
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        items.remove(i);
        changed = true;
    } else if let Some(i) = swap {
        items.swap(i - 1, i);
        changed = true;
    }
    if ui
        .add_enabled(
            !choices.is_empty() || boundary && !edges.is_empty(),
            egui::Button::new(format!("Add {}", label.to_lowercase())),
        )
        .clicked()
    {
        let choice = choices
            .iter()
            .find(|s| {
                !items.iter().any(|v| {
                    v.as_str() == Some(s) || v.get("feature").and_then(Value::as_str) == Some(s)
                })
            })
            .or(choices.first());
        if boundary {
            items.push(
                choice
                    .map(|id| json!({"kind":"curve","feature":id}))
                    .unwrap_or_else(|| json!({"kind":"edge","feature":edges[0],"edge":edge()})),
            );
        } else if let Some(id) = choice {
            items.push(json!(id));
        }
        changed = true;
    }
    changed
}
impl FeatureForm {
    pub(crate) fn apply_surface_edge(&mut self, path: &str, edge: Value) -> Result<(), String> {
        let mut v = self.geometry_value()?;
        *v.pointer_mut(path)
            .ok_or("The boundary changed. Choose it again.")? = edge;
        self.replace_value(v);
        Ok(())
    }
    pub(super) fn surface_options(&mut self, ui: &mut egui::Ui) -> Result<(), String> {
        let Some(tool) = SurfaceTool::ALL
            .into_iter()
            .find(|t| self.value["op"].as_str() == Some(t.op()))
        else {
            return Ok(());
        };
        let ids = Inputs::new(&self.expected.parts[&self.part].geometry, &self.id);
        let mut v = self.geometry_value()?;
        let mut changed = false;
        for (key, label, options) in [
            ("profile", "Profile", &ids.profiles),
            ("axis", "Reference axis", &ids.axes),
            ("surface", "Surface", &ids.surfaces),
            ("body", "Solid", &ids.solids),
        ] {
            if v[key].is_string() {
                changed |= select(ui, label, &mut v[key], options);
            }
        }
        if tool == SurfaceTool::Sweep && v["path"].is_string() {
            changed |= select(ui, "Sweep path", &mut v["path"], &ids.paths);
        }
        if v.get("tool").is_some() {
            let options = if tool == SurfaceTool::Trim {
                ids.tools
                    .iter()
                    .filter(|id| v["surface"].as_str() != Some(id))
                    .cloned()
                    .collect()
            } else {
                ids.surfaces.clone()
            };
            changed |= select(ui, "Cutting tool", &mut v["tool"], &options);
        }
        if tool == SurfaceTool::Extrude {
            let mut enabled = !v["direction"].is_null();
            if ui.checkbox(&mut enabled, "Set direction").changed() {
                if enabled {
                    v["direction"] = json!([0, 0, 1]);
                } else {
                    v.as_object_mut().unwrap().remove("direction");
                }
                changed = true;
            }
        }
        if tool == SurfaceTool::Stitch {
            let mut enabled = !v["tolerance"].is_null();
            if ui
                .checkbox(&mut enabled, "Set stitch tolerance (default 0.001 mm)")
                .changed()
            {
                if enabled {
                    v["tolerance"] = json!(0.001);
                } else {
                    v.as_object_mut().unwrap().remove("tolerance");
                }
                changed = true;
            }
        }
        let edges = ids
            .solids
            .iter()
            .chain(&ids.surfaces)
            .cloned()
            .collect::<Vec<_>>();
        for (key, label, options) in [
            ("sections", "Section", &ids.profiles),
            ("surfaces", "Surface", &ids.surfaces),
            ("boundary", "Boundary", &ids.profiles),
            ("tangent_to", "Tangent support", &edges),
        ] {
            if let Some(items) = v.get_mut(key).and_then(Value::as_array_mut) {
                changed |= list(ui, key, label, items, options, key == "boundary", &edges);
            }
        }
        if tool == SurfaceTool::Extend && ui.button("Pick surface edge").clicked() {
            self.surface_pick = Some((
                v["surface"].as_str().unwrap_or_default().into(),
                "/edge".into(),
            ));
        }
        if tool == SurfaceTool::Patch
            && let Some(rows) = v["boundary"].as_array()
        {
            for (i, row) in rows.iter().enumerate() {
                if row["kind"] == "edge"
                    && ui.button(format!("Pick boundary edge {}", i + 1)).clicked()
                {
                    self.surface_pick = Some((
                        row["feature"].as_str().unwrap_or_default().into(),
                        format!("/boundary/{i}/edge"),
                    ));
                }
            }
        }
        if changed {
            self.replace_value(v);
        }
        Ok(())
    }
}
