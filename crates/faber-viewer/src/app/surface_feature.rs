//! Inspect an intermediate surface without changing the solid output.
use super::*;
use crate::feature_form::surfaces::{SurfaceTool, surface};
use faber_core::pick_selector::PickTopology;
use faber_model::{Geometry, Scalar};
use faber_render::SurfacePaint;
use faber_wiring::surface_feature::{Display, display};
use std::sync::{Arc, Weak};

struct Output {
    item: Display,
    topology: PickTopology,
    paint: SurfacePaint,
}
pub(super) struct Preview {
    pub(super) part: String,
    pub(super) id: String,
    source: Weak<Vec<Body>>,
    job: Option<mpsc::Receiver<Result<Output, String>>>,
    shown: Option<(Display, PickTopology)>,
    current: bool,
    error: Option<String>,
    edge_key: Option<String>,
    #[cfg(test)]
    pub(super) gate: Option<mpsc::Receiver<()>>,
}
fn owner<'a>(data: &'a ProjectData, id: &str) -> Option<&'a Geometry<Scalar>> {
    let part = data.parts.get(id)?;
    if let Geometry::Instance { of } = &part.geometry {
        Some(&data.parts.get(of)?.geometry)
    } else {
        Some(&part.geometry)
    }
}
fn geometry<'a>(data: &'a ProjectData, part: &str, id: &str) -> Option<&'a Geometry<Scalar>> {
    let Geometry::History { features, .. } = owner(data, part)? else {
        return None;
    };
    features
        .iter()
        .find(|f| f.id == id && surface(features, f))
        .map(|f| &f.geometry)
}
impl Shell {
    pub(super) fn clear_surface_edge_request(&mut self) {
        if let Some(p) = &mut self.intermediate {
            p.edge_key = None;
        }
    }
    pub(super) fn replacing_solid(&self, part: &str) -> bool {
        self.intermediate
            .as_ref()
            .is_some_and(|p| p.part == part && p.shown.is_some())
    }
    pub(super) fn picking_surface_edge(&self) -> bool {
        self.feature_draft.is_some()
            && self
                .intermediate
                .as_ref()
                .is_some_and(|p| p.edge_key.is_some())
    }
    pub(super) fn surface_pick_body(&self) -> Option<&Body> {
        let p = self.intermediate.as_ref()?;
        if !p.current || p.edge_key.is_none() || self.hidden.contains(&p.part) {
            return None;
        }
        p.shown.as_ref().map(|(display, _)| &display.body)
    }
    pub(super) fn close_surface_feature(&mut self) {
        if self.intermediate.take().is_some() {
            self.gpu.intermediate = None;
            self.clear_entity_picks();
            self.dirty = true;
        }
    }
    pub(super) fn open_surface_feature(&mut self, part: String, id: String) {
        if self
            .intermediate
            .as_ref()
            .is_some_and(|p| p.part == part && p.id == id)
        {
            return;
        }
        self.close_surface_feature();
        self.clear_entity_picks();
        self.close_sheet();
        self.intermediate = Some(Preview {
            part,
            id,
            source: Weak::new(),
            job: None,
            shown: None,
            current: false,
            error: None,
            edge_key: None,
            #[cfg(test)]
            gate: None,
        });
    }
    pub(super) fn surface_feature_menu(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected.clone();
        ui.menu_button("Surface tools", |ui| {
            let editable = selected
                .as_ref()
                .and_then(|id| {
                    Some(
                        faber_core::pick_selector::editable_part(id, self.data.parts.get(id)?)
                            .is_ok(),
                    )
                })
                .unwrap_or(false);
            if let Some(s) = &self.design_source_name {
                ui.label(format!("Edit the surfaces in {s}."));
            }
            for t in SurfaceTool::ALL {
                if ui
                    .add_enabled(
                        self.design_source_name.is_none() && editable && self.pending.is_none(),
                        egui::Button::new(t.label()),
                    )
                    .clicked()
                {
                    ui.close();
                    self.run_command(ui.ctx(), keys::Command::Feature(ToolKind::Surface(t)));
                }
            }
        });
        // An occurrence has the feature list of its source in its own placement.
        if let Some(part) = selected
            && matches!(
                self.data.parts.get(&part).map(|p| &p.geometry),
                Some(Geometry::Instance { .. })
            )
            && let Some(Geometry::History { features, .. }) = owner(&self.data, &part)
        {
            let pick = ui
                .menu_button("Inspect source surfaces", |ui| {
                    for f in features.iter().filter(|f| surface(features, f)) {
                        if ui.button(&f.id).clicked() {
                            ui.close();
                            return Some(f.id.clone());
                        }
                    }
                    None
                })
                .inner
                .flatten();
            if let Some(id) = pick {
                self.open_surface_feature(part, id);
            }
        }
    }
    pub(super) fn intermediate_body<'a>(&'a self, body: &'a Body) -> &'a Body {
        self.intermediate
            .as_ref()
            .filter(|p| p.part == body.id)
            .and_then(|p| p.shown.as_ref())
            .map_or(body, |(d, _)| &d.body)
    }
    pub(super) fn surface_feature_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mut p) = self.intermediate.take() else {
            return false;
        };
        let Some(g) = geometry(&self.data, &p.part, &p.id)
            .filter(|_| self.selected.as_deref() == Some(p.part.as_str()))
        else {
            self.gpu.intermediate = None;
            self.dirty = true;
            self.clear_entity_picks();
            return false;
        };
        if !p.source.ptr_eq(&Arc::downgrade(&self.bodies)) {
            p.current = false;
            p.error = None;
            p.source = Arc::downgrade(&self.bodies);
            let (tx, rx) = mpsc::channel();
            p.job = Some(rx);
            let data = self.data.clone();
            let root = self.root.clone();
            let part = p.part.clone();
            let id = p.id.clone();
            let device = self.gpu.device.clone();
            let ctx = ui.ctx().clone();
            #[cfg(test)]
            let gate = p.gate.take();
            std::thread::spawn(move || {
                let result = (|| {
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        gate.recv()
                            .map_err(|_| "The surface worker stopped.".to_owned())?;
                    }
                    let item = display(&root, &data, &part, &id)?;
                    let paint = SurfacePaint::intermediate(&device, &item.body, &item.boundaries)?;
                    let topology = faber_wiring::pick_topology(&item.body, &data)?;
                    Ok(Output {
                        item,
                        topology,
                        paint,
                    })
                })();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        if let Some(job) = &p.job {
            let result = match job.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The surface worker stopped.".into()))
                }
            };
            if let Some(r) = result {
                p.job = None;
                match r {
                    Ok(o) => {
                        p.current = true;
                        p.shown = Some((o.item, o.topology));
                        self.gpu.intermediate = Some(o.paint);
                        self.dirty = true;
                    }
                    Err(e) => p.error = Some(e),
                }
            }
        }
        ui.heading("Surface feature");
        ui.label(format!("{} · {}", p.part, p.id));
        let editable = self.design_source_name.is_none()
            && faber_core::pick_selector::editable_part(&p.part, &self.data.parts[&p.part]).is_ok();
        let mut open = None;
        let mut thicken = false;
        if p.edge_key.is_some() && self.feature_draft.is_some() {
            ui.label("Click an edge on the displayed surface.");
            if ui.button("Cancel edge pick").clicked() {
                p.edge_key = None;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Show solid").clicked() {
                open = Some(false);
            }
            if ui
                .add_enabled(
                    editable && p.current && self.pending.is_none(),
                    egui::Button::new("Edit surface"),
                )
                .clicked()
            {
                open = Some(true);
            }
            thicken = ui
                .add_enabled(
                    editable && p.current && self.pending.is_none(),
                    egui::Button::new("Thicken surface"),
                )
                .clicked();
        });
        if let Some((view, _)) = &p.shown {
            ui.label(format!("Area {:.2} mm²", view.area_mm2));
            ui.label(format!("{} open edges", view.boundaries.ranges.len()));
        }
        if let Some(source) = &self.design_source_name {
            ui.label(format!(
                "Edit the surface in {source}, then build the project."
            ));
        }
        if let Some(message) = &p.error {
            ui.colored_label(Color32::LIGHT_RED, message);
            if ui.button("Read surface again").clicked() {
                p.source = Weak::new();
            }
        } else if !p.current {
            ui.weak(if p.shown.is_some() {
                "Updating the displayed surface…"
            } else {
                "Reading the surface…"
            });
        }
        if let Some(edit) = open {
            if !edit {
                self.gpu.intermediate = None;
                self.dirty = true;
                self.clear_entity_picks();
                return true;
            }
            let form = crate::feature_form::FeatureForm::new(
                self.data.clone(),
                p.part.clone(),
                p.id.clone(),
                g,
            );
            match form {
                Ok(f) => self.request_window(Draft::Feature(f)),
                Err(e) => self.error = Some(e),
            }
        }
        self.intermediate = Some(p);
        if thicken {
            self.run_command(ui.ctx(), keys::Command::Feature(ToolKind::Thicken));
        }
        true
    }
    pub(super) fn start_surface_edge_pick(
        &mut self,
        part: String,
        surface_id: String,
        key: String,
    ) {
        if geometry(&self.data, &part, &surface_id).is_some() {
            self.open_surface_feature(part, surface_id);
            if let Some(p) = &mut self.intermediate {
                p.edge_key = Some(key);
            }
        } else if self
            .data
            .parts
            .get(&part)
            .is_some_and(|p| match &p.geometry {
                Geometry::History { output, .. } => *output == surface_id,
                _ => surface_id == "source",
            })
        {
            self.close_surface_feature();
            if let Some(f) = &mut self.feature_draft {
                f.solid_edge_path = Some(key);
            }
            self.enter_pick_tool();
        } else {
            self.window.error=Some("Pick an edge on the solid output or on a surface. Enter a selector for an earlier solid.".into());
        }
    }
    pub(super) fn surface_input_click(&mut self, rect: egui::Rect, pos: egui::Pos2) -> bool {
        let Some(p) = &self.intermediate else {
            return false;
        };
        let Some(key) = p.edge_key.clone().filter(|_| self.feature_draft.is_some()) else {
            if self.pick_body(rect, pos).as_deref() == Some(&p.part) {
                return true;
            }
            self.close_surface_feature();
            return false;
        };
        if !p.current || self.pending.is_some() || self.hidden.contains(&p.part) {
            return true;
        }
        let Some((display, topo)) = p.shown.as_ref() else {
            return true;
        };
        let result = self
            .edge_at(&display.body, rect, pos)
            .ok_or("Pick an edge on the displayed surface.".to_owned())
            .and_then(|hit| topo.edge_selector(&[hit.edge]))
            .and_then(|s| serde_json::to_value(s).map_err(|e| e.to_string()))
            .and_then(|s| {
                self.feature_draft
                    .as_mut()
                    .unwrap()
                    .apply_surface_edge(&key, s)
            });
        match result {
            Ok(()) => {
                self.intermediate.as_mut().unwrap().edge_key = None;
                self.window.dirty = true;
                self.window.error = None;
            }
            Err(e) => self.window.error = Some(e),
        }
        true
    }
}
