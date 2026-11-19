use super::*;
use faber_model::view::NamedView;
use faber_render::studio::{StudioList, StudioOptions};
use std::sync::{Arc, Weak};

mod animation;
use animation::Export;

#[derive(Default)]
pub(super) struct Views {
    pub open: bool,
    name: String,
    active: String,
    pub studio: bool,
    pub style: StudioOptions,
    pub display: Option<StudioList>,
    source: Weak<Vec<Body>>,
    revision: u64,
    #[cfg(test)]
    pub studio_gate: Option<mpsc::Receiver<()>>,
    #[cfg(test)]
    pub uploads: usize,
    job: Option<mpsc::Receiver<Result<StudioList, String>>>,
    studio_error: Option<String>,
    pub explode: f64,
    pub section_override: Option<String>,
    #[cfg(test)]
    pub export_gate: Option<mpsc::Receiver<()>>,
    offset_source: Weak<Vec<Body>>,
    offsets: Vec<faber_core::explode::Offset>,
    posed_factor: f64,
    motion: BTreeMap<String, glam::Mat4>,
    pub poses: BTreeMap<String, glam::Mat4>,
    message: Option<String>,
    export: Export,
}

impl Shell {
    pub(super) fn update_pictures(&mut self, ctx: &egui::Context) {
        let state = &mut self.views;
        if state.explode != 0. {
            let source = Arc::downgrade(&self.bodies);
            let changed = !source.ptr_eq(&state.offset_source);
            if changed {
                state.offset_source = source;
                match faber_core::explode::offsets(&self.bodies, &self.data.motion, 1.) {
                    Ok(offsets) => state.offsets = offsets,
                    Err(e) => {
                        state.offsets.clear();
                        state.message = Some(e);
                    }
                }
            }
            if changed || state.explode != state.posed_factor || state.motion != self.motion.poses {
                state.motion.clone_from(&self.motion.poses);
                state.poses.clone_from(&state.motion);
                for item in &state.offsets {
                    let v = (glam::DVec3::from(item.vector) * state.explode).as_vec3();
                    if v == glam::Vec3::ZERO {
                        continue;
                    }
                    let pose = state
                        .poses
                        .entry(item.part.clone())
                        .or_insert(glam::Mat4::IDENTITY);
                    *pose = glam::Mat4::from_translation(v) * *pose;
                }
                state.posed_factor = state.explode;
                self.dirty = true;
            }
        } else {
            state.posed_factor = 0.;
        }
        if !state.studio {
            return;
        }
        let source = Arc::downgrade(&self.bodies);
        let revision = self.data.revision;
        if let Some(rx) = &state.job {
            let v = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The studio worker stopped.".into()))
                }
            };
            if let Some(result) = v {
                state.job = None;
                if state.source.ptr_eq(&source) && state.revision == revision {
                    match result {
                        Ok(list) => state.display = Some(list),
                        Err(e) => state.studio_error = Some(e),
                    }
                    self.dirty = true;
                }
            }
        }
        if state.source.ptr_eq(&source) && state.revision == revision {
            return;
        }
        state.display = None;
        state.studio_error = None;
        if state.job.is_some() {
            return;
        }
        state.source = source;
        state.revision = revision;
        let bodies = self.bodies.clone();
        let data = self.data.clone();
        let load = faber_render::studio::uploader(&self.gpu);
        #[cfg(test)]
        {
            state.uploads += 1;
        }
        let (tx, rx) = mpsc::channel();
        state.job = Some(rx);
        let ctx = ctx.clone();
        #[cfg(test)]
        let gate = state.studio_gate.take();
        std::thread::spawn(move || {
            #[cfg(test)]
            if let Some(gate) = gate
                && gate.recv().is_err()
            {
                return;
            }
            let result = faber_wiring::studio::surfaces(&data, &bodies)
                .and_then(|materials| load(&bodies, &materials));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn exploded_camera(&self) -> Option<Camera> {
        if self.views.explode == 0. {
            return None;
        }
        let poses = self.move_poses();
        let mut low = glam::Vec3::splat(f32::INFINITY);
        let mut high = -low;
        for body in self.bodies.iter().filter(|b| !self.hidden.contains(&b.id)) {
            let pose = poses.get(&body.id).copied().unwrap_or(glam::Mat4::IDENTITY);
            for i in 0..8 {
                let p = glam::Vec3::from_array(std::array::from_fn(|j| {
                    body.bounds[(i >> j) & 1][j] as f32
                }));
                let q = pose.transform_point3(p);
                low = low.min(q);
                high = high.max(q);
            }
        }
        low.is_finite().then(|| Camera {
            target: (low + high) * 0.5,
            distance: (high - low).length().max(1.) * 1.8,
            ..self.camera
        })
    }

    fn snapshot_view(
        &self,
    ) -> (
        NamedView,
        Option<(String, faber_model::SectionAnalysis<faber_model::Scalar>)>,
    ) {
        let mut view = NamedView {
            camera: self.camera.to_named(),
            hidden: self.hidden.iter().cloned().collect(),
            section: self.section_selected.clone().filter(|_| self.section_on),
            explode: (self.views.explode != 0.).then_some(self.views.explode),
            pose: None,
            joints: self.motion.values.clone(),
            style: self.views.studio.then(|| "studio".into()),
            note: String::new(),
        };
        let section = if self.section_on && view.section.is_none() {
            let id = crate::analysis::new_id(&self.data);
            let mut section = crate::analysis::from_plane(self.plane);
            section.parts = self.section_scope.iter().cloned().collect();
            view.section = Some(id.clone());
            Some((id, section))
        } else {
            None
        };
        (view, section)
    }
    fn save_named_view(&mut self, ctx: &egui::Context) {
        let name = self.views.name.trim().to_owned();
        let (mut view, section) = self.snapshot_view();
        if let Some(previous) = self.data.views.get(&name) {
            view.note.clone_from(&previous.note);
        }
        let mut edits = Vec::new();
        if let Some((id, section)) = section {
            edits.push(faber_model::Edit::SetSection { id, section });
        }
        edits.push(faber_model::Edit::SetView {
            name: name.clone(),
            view,
        });
        self.views.active = name;
        self.views.message = None;
        self.save_sections(ctx, edits);
    }

    /// Applies a saved view by name, as the Apply button does.
    pub(super) fn apply_named_view(
        &mut self,
        ctx: &egui::Context,
        name: &str,
    ) -> Result<(), String> {
        if !self.data.views.contains_key(name) {
            return Err(format!("View {name} was not found."));
        }
        self.views.active = name.to_owned();
        self.apply_view(ctx)
    }

    fn apply_view(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let id = &self.views.active;
        let view = self.data.views.get(id).ok_or("Select a named view.")?;
        let camera = Camera::from_named(&view.camera)?;
        let pose = faber_wiring::views::pose(&self.data, id)?;
        if !pose.is_empty() {
            self.data.motion.solve_pose(&pose)?;
        }
        let mut notices = Vec::new();
        self.structure.clear_isolation();
        self.hidden = view
            .hidden
            .iter()
            .filter(|part| {
                let found = self.data.parts.contains_key(*part);
                if !found {
                    notices.push(format!("View {id}: part {part} was not found."));
                }
                found
            })
            .cloned()
            .collect();
        self.section_selected = view.section.clone().filter(|section| {
            let present = self.data.sections.contains_key(section);
            if !present {
                notices.push(format!("View {id}: section {section} was not found."));
            }
            present
        });
        self.views.section_override = self.section_selected.clone();
        self.section_pending = None;
        self.section_drag = None;
        self.section_pivot = None;
        self.section_anchor = None;
        self.section_basis = None;
        self.section_scope.clear();
        self.section_on = false;
        self.cap_plane = None;
        self.views.studio = view.style.as_deref() == Some("studio");
        self.views.explode = view.explode.unwrap_or(0.);
        self.views.message = (!notices.is_empty()).then(|| notices.join("\n"));
        self.camera = camera;
        if pose.is_empty() {
            self.reset_motion_pose();
        } else {
            self.initial_pose(pose);
        }
        self.sync_section(ctx);
        self.dirty = true;
        Ok(())
    }

    pub(super) fn views_panel(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.views.open {
            return false;
        }
        ui.heading("Views");
        if ui.button("Back to model").clicked() {
            self.views.open = false;
            return true;
        }
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!self.views.studio, "Technical")
                .clicked()
            {
                self.views.studio = false;
                self.dirty = true;
            }
            if ui.selectable_label(self.views.studio, "Studio").clicked() {
                self.views.studio = true;
                self.dirty = true;
            }
        });
        if self.views.studio {
            if ui
                .checkbox(&mut self.views.style.ground, "Ground shadow")
                .changed()
            {
                self.dirty = true;
            }
            ui.horizontal(|ui| {
                let label = ui.label("Background");
                if ui
                    .color_edit_button_srgb(&mut self.views.style.background)
                    .labelled_by(label.id)
                    .changed()
                {
                    self.dirty = true;
                }
            });
            ui.horizontal(|ui| {
                let label = ui.label("Exposure");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.views.style.exposure)
                            .range(0.05..=10.)
                            .speed(0.02),
                    )
                    .labelled_by(label.id)
                    .changed()
                {
                    self.dirty = true;
                }
            });
            if let Some(e) = &self.views.studio_error {
                ui.colored_label(Color32::LIGHT_RED, e);
            } else if self.views.display.is_some() {
                ui.weak("Studio ready");
            } else {
                ui.weak("Preparing studio lighting…");
            }
        }
        if ui
            .add(egui::Slider::new(&mut self.views.explode, 0.0..=1.0).text("Explode"))
            .changed()
        {
            self.dirty = true;
        }
        ui.separator();
        for id in self.data.views.keys() {
            if ui.selectable_label(self.views.active == *id, id).clicked() {
                self.views.active = id.clone();
                self.views.name = id.clone();
            }
        }
        let selected = self.data.views.contains_key(&self.views.active);
        if ui
            .add_enabled(selected, egui::Button::new("Apply"))
            .clicked()
            && let Err(e) = self.apply_view(ui.ctx())
        {
            self.views.message = Some(e);
        }
        let label = ui.label("View name");
        ui.text_edit_singleline(&mut self.views.name)
            .labelled_by(label.id);
        let valid = faber_core::names::valid_view(&self.views.name);
        let saving = self.saving || self.pending.is_some();
        let caption = if self.data.views.contains_key(self.views.name.trim()) {
            "Update view"
        } else {
            "Save view"
        };
        if ui
            .add_enabled(valid && !saving, egui::Button::new(caption))
            .clicked()
        {
            self.save_named_view(ui.ctx());
        }
        if !valid && !self.views.name.is_empty() {
            ui.weak("Use 1 to 100 visible characters with no spaces at either end.");
        }
        if let Some(e) = &self.views.message {
            ui.label(e);
        }
        ui.separator();
        self.animation_panel(ui);
        true
    }
}
