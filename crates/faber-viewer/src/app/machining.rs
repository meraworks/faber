//! Preview the selected setup and its verified stock in the assembly frame.
use super::*;
use faber_model::machining::ToolKind;
use faber_render::machining::Display;
use faber_wiring::{machining as cam, machining_image};
use std::sync::{Arc, Weak};

struct View {
    display: Display,
    times: Vec<(String, f64)>,
    gouge: f64,
    excess: f64,
    removed: f64,
    collisions: usize,
    warnings: Vec<String>,
    limits: Vec<String>,
}
type Version = (u64, Option<(SystemTime, u64)>);
struct Job {
    setup: String,
    source: Weak<Vec<Body>>,
    version: Option<Version>,
    rx: mpsc::Receiver<Result<View, String>>,
}
pub(super) struct Panel {
    setup: String,
    source: Weak<Vec<Body>>,
    version: Option<Version>,
    job: Option<Job>,
    view: Option<View>,
    error: Option<String>,
    current: bool,
    simulate: bool,
    operation: Option<String>,
    previous_camera: Camera,
    fit: bool,
    #[cfg(test)]
    pub(super) gate: Option<mpsc::Receiver<()>>,
}
fn tool_name(k: ToolKind) -> &'static str {
    match k {
        ToolKind::Flat => "Flat mill",
        ToolKind::Ball => "Ball mill",
        ToolKind::Bull => "Bull mill",
        ToolKind::Drill => "Drill",
        ToolKind::Chamfer => "Chamfer",
        ToolKind::Laser => "Laser",
    }
}
impl Panel {
    fn select(&mut self, setup: String) {
        self.setup = setup;
        self.source = Weak::new();
        self.view = None;
        self.error = None;
        self.current = false;
        self.fit = true;
        self.operation = None;
        self.simulate = false;
    }
    fn update(&mut self, app: &mut Shell, ctx: &egui::Context) {
        let source = Arc::downgrade(&app.bodies);
        let version = Some((app.data.revision, app.modified));
        if app.pending.is_some() {
            self.current = false;
            return;
        }
        if self.version != version || !self.source.ptr_eq(&source) {
            self.current = false;
            self.error = None;
        }
        if let Some(job) = &self.job {
            let result = match job.rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The machining worker stopped.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(v) = result {
                let job = self.job.take().unwrap();
                if job.setup == self.setup && job.source.ptr_eq(&source) && job.version == version {
                    match v {
                        Ok(view) => {
                            if self.fit {
                                let aspect = app
                                    .target
                                    .as_ref()
                                    .map_or(1., |t| t.width as f32 / t.height as f32);
                                app.camera = view.display.camera(aspect);
                                self.fit = false;
                            }
                            if self
                                .operation
                                .as_ref()
                                .is_some_and(|op| !view.times.iter().any(|(id, _)| id == op))
                            {
                                self.operation = None;
                            }
                            self.view = Some(view);
                            self.current = true;
                            app.dirty = true;
                        }
                        Err(e) => {
                            self.error = Some(e);
                            self.current = false;
                            app.dirty = true;
                        }
                    }
                }
            }
        }
        if self.job.is_some()
            || self.source.ptr_eq(&source) && self.version == version
            || self.setup.is_empty()
        {
            return;
        }
        let root = app.root.clone();
        let data = app.data.clone();
        let setup = self.setup.clone();
        let ctx = ctx.clone();
        let device = app.gpu.device.clone();
        let (tx, rx) = mpsc::channel();
        self.source = source.clone();
        self.version = version;
        self.job = Some(Job {
            setup: setup.clone(),
            source,
            version,
            rx,
        });
        #[cfg(test)]
        let gate = self.gate.take();
        std::thread::spawn(move || {
            let result = (|| {
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate.recv()
                        .map_err(|_| "The machining worker stopped.".to_owned())?;
                }
                let run = cam::calculate(&root, &data, &setup, None)?;
                let display = machining_image::scene(&run)?.upload(&device)?;
                let v = &run.verification;
                Ok(View {
                    display,
                    times: run
                        .reports
                        .iter()
                        .map(|r| (r.operation.clone(), r.minutes))
                        .collect(),
                    gouge: v.gouge_mm,
                    excess: v.max_excess_mm,
                    removed: v.removed_mm3,
                    collisions: v.rapid_collision_count,
                    warnings: v.warnings.clone(),
                    limits: run.limits,
                })
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl Shell {
    pub(super) fn open_machining(&mut self) {
        let sets = &self.data.machining.setups;
        let setup = sets
            .iter()
            .find(|(_, s)| Some(s.part.as_str()) == self.selected.as_deref())
            .or_else(|| sets.first_key_value())
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        self.machining = Some(Panel {
            setup,
            source: Weak::new(),
            version: None,
            job: None,
            view: None,
            error: None,
            current: false,
            simulate: false,
            operation: None,
            previous_camera: self.camera,
            fit: true,
            #[cfg(test)]
            gate: None,
        });
        self.dirty = true;
    }
    pub(super) fn machining_camera(&self) -> Option<Camera> {
        let p = self.machining.as_ref()?.view.as_ref()?;
        let aspect = self
            .target
            .as_ref()
            .map_or(1., |t| t.width as f32 / t.height as f32);
        Some(p.display.camera(aspect))
    }
    pub(super) fn machining_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mut p) = self.machining.take() else {
            return false;
        };
        ui.heading("Manufacture");
        if ui.button("Back to model").clicked() {
            self.camera = p.previous_camera;
            self.dirty = true;
            return true;
        }
        let sets = &self.data.machining.setups;
        if !sets.contains_key(&p.setup) && (!p.setup.is_empty() || !sets.is_empty()) {
            p.select(sets.keys().next().cloned().unwrap_or_default());
            self.dirty = true;
        }
        if sets.is_empty() {
            p.update(self, ui.ctx());
            ui.label("Add a machining setup to the project.");
            self.machining = Some(p);
            return true;
        }
        let mut setup = p.setup.clone();
        egui::ComboBox::from_label("Setup")
            .selected_text(&setup)
            .truncate()
            .show_ui(ui, |ui| {
                for id in sets.keys() {
                    ui.selectable_value(&mut setup, id.clone(), id);
                }
            });
        if setup != p.setup {
            p.select(setup);
            self.dirty = true;
        }
        p.update(self, ui.ctx());
        let settings = &self.data.machining;
        ui.label(format!("Part {}", settings.setups[&p.setup].part));
        ui.heading("Tools");
        for (id, t) in &settings.tools {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("T{}", t.number));
                ui.label(id);
            });
            ui.weak(format!("{} · Ø{:.2} mm", tool_name(t.kind), t.diameter))
                .on_hover_text(format!("{} flutes · flute length {:.2} mm\nFeed {} mm/min · plunge {} mm/min\nSpindle {} rpm",t.flutes,t.flute_length,t.feed_mm_min,t.plunge_mm_min,t.spindle_rpm));
        }
        ui.heading("Operations");
        for op in settings.operations.iter().filter(|op| op.setup == p.setup) {
            let selected = p.operation.as_ref() == Some(&op.id);
            if ui.selectable_label(selected, &op.id).clicked() {
                p.operation = (!selected).then(|| op.id.clone());
                self.dirty = true;
            }
            let time = p
                .view
                .as_ref()
                .filter(|_| p.current)
                .and_then(|v| v.times.iter().find(|(id, _)| id == &op.id));
            ui.weak(time.map_or_else(
                || "Operation time …".into(),
                |(_, m)| format!("Operation time {m:.2} min"),
            ));
        }
        ui.separator();
        let can_simulate = p.view.is_some() && p.current;
        if ui
            .add_enabled(
                can_simulate,
                egui::Checkbox::new(&mut p.simulate, "Simulate"),
            )
            .changed()
        {
            self.dirty = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(Color32::from_rgb(247, 196, 47), "Rapid");
            ui.colored_label(Color32::from_rgb(65, 153, 255), "Feed");
        });
        if let Some(e) = &p.error {
            ui.colored_label(Color32::LIGHT_RED, e);
            if p.view.is_some() {
                ui.weak("Showing previous toolpaths.");
            }
            if ui.button("Calculate again").clicked() {
                p.source = Weak::new();
            }
        } else if !p.current {
            ui.weak(if p.view.is_some() {
                "Updating the previous toolpaths…"
            } else {
                "Calculating toolpaths…"
            });
        } else if let Some(v) = &p.view {
            ui.label("Toolpaths ready");
            if p.simulate {
                ui.heading("Cut stock");
                ui.label(format!("Gouge {:.2} mm", v.gouge));
                ui.label(format!("Excess {:.2} mm", v.excess));
                ui.label(format!("Removed {:.1} mm³", v.removed));
                if v.collisions > 0 {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        format!("Rapid collisions {}", v.collisions),
                    );
                }
                ui.collapsing("Verification limits", |ui| {
                    for limit in &v.limits {
                        ui.label(limit);
                    }
                });
                for w in &v.warnings {
                    ui.label(w);
                }
            }
        }
        self.machining = Some(p);
        true
    }
    pub(super) fn machining_canvas(
        &mut self,
        ui: &mut egui::Ui,
        rs: &egui_wgpu::RenderState,
    ) -> bool {
        if self.machining.is_none() {
            return false;
        }
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Model viewport"));
        self.ensure_target(ui, rs, rect);
        let delta = ui.input(|i| i.pointer.delta());
        let shift = ui.input(|i| i.modifiers.shift);
        if response.dragged_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Middle) && shift
        {
            self.camera.yaw -= delta.x * 0.35;
            self.camera.pitch = (self.camera.pitch + delta.y * 0.35).clamp(-89., 89.);
            self.dirty = true;
        } else if response.dragged_by(egui::PointerButton::Middle) {
            let (r, u) = self.camera.screen_axes();
            self.camera.target +=
                (-r * delta.x + u * delta.y) * self.camera.distance / rect.height();
            self.dirty = true;
        }
        if response.hovered()
            && let Some(pos) = response.hover_pos()
        {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                let ndc = [
                    2. * (pos.x - rect.left()) / rect.width() - 1.,
                    1. - 2. * (pos.y - rect.top()) / rect.height(),
                ];
                self.camera
                    .zoom_at(rect.aspect_ratio(), ndc, (-scroll * 0.002).exp());
                self.dirty = true;
            }
        }
        if response.double_clicked_by(egui::PointerButton::Middle) {
            self.fit();
        }
        let p = self.machining.as_ref().unwrap();
        if let (Some(v), Some(target), Some(texture)) = (&p.view, &self.target, self.texture) {
            if self.dirty {
                let chosen = p
                    .operation
                    .as_ref()
                    .and_then(|op| v.times.iter().position(|(id, _)| id == op));
                v.display
                    .draw(&self.gpu, target, self.camera, p.simulate, chosen);
                self.dirty = false;
            }
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        self.view_cube(ui, rect);
        true
    }
}
