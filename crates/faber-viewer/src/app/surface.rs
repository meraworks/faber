use super::*;
use faber_core::surface::{CurvatureMode, DraftClass, access_colour, draft_colour};
use faber_render::SurfacePaint;
use faber_wiring::surface::{Request, centre_of_mass};
use std::sync::{Arc, Weak};

#[derive(Copy, Clone, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Off,
    Curvature,
    Draft,
    Access,
    Zebra,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Curvature => "Curvature",
            Self::Draft => "Draft",
            Self::Access => "Access",
            Self::Zebra => "Zebra",
        }
    }
}

struct Reading {
    range: Option<[f64; 2]>,
    report: serde_json::Value,
}

enum Output {
    Surface(SurfacePaint, Reading),
    Centre([f64; 3]),
}

enum Task {
    Surface(Request),
    Centre,
}

struct Job {
    centre: bool,
    generation: u64,
    receiver: mpsc::Receiver<Result<Output, String>>,
}

pub(super) struct SurfacePanel {
    mode: Mode,
    part: Option<String>,
    geometry: Weak<Vec<Body>>,
    curvature: CurvatureMode,
    tool_limit: bool,
    tool_radius: f64,
    pull: [f64; 3],
    minimum: f64,
    access: [bool; 6],
    access_vector: Option<[f64; 3]>,
    scene: bool,
    count: u32,
    show_centre: bool,
    centre: Option<[f64; 3]>,
    reading: Option<Reading>,
    generation: u64,
    poses: BTreeMap<String, glam::Mat4>,
    pending: bool,
    centre_pending: bool,
    job: Option<Job>,
    error: Option<String>,
    centre_error: Option<String>,
}

impl Default for SurfacePanel {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            part: None,
            geometry: Weak::new(),
            curvature: CurvatureMode::Mean,
            tool_limit: false,
            tool_radius: 3.,
            pull: [0., 0., 1.],
            minimum: 1.,
            access: [false, false, true, false, false, false],
            access_vector: None,
            scene: false,
            count: 12,
            show_centre: false,
            centre: None,
            reading: None,
            generation: 0,
            poses: Default::default(),
            pending: false,
            centre_pending: false,
            job: None,
            error: None,
            centre_error: None,
        }
    }
}

const AXES: [(&str, [f64; 3]); 6] = [
    ("X", [1., 0., 0.]),
    ("Y", [0., 1., 0.]),
    ("Z", [0., 0., 1.]),
    ("−X", [-1., 0., 0.]),
    ("−Y", [0., -1., 0.]),
    ("−Z", [0., 0., -1.]),
];

impl SurfacePanel {
    fn request(&self) -> Option<Request> {
        match self.mode {
            Mode::Curvature => Some(Request::Curvature {
                mode: self.curvature,
                tool_radius_mm: self.tool_limit.then_some(self.tool_radius),
            }),
            Mode::Draft => Some(Request::Draft {
                pull: self.pull,
                min_deg: self.minimum,
            }),
            Mode::Access => Some(Request::Access {
                directions: AXES
                    .iter()
                    .zip(self.access)
                    .filter(|(_, on)| *on)
                    .map(|((_, v), _)| *v)
                    .chain(self.access_vector)
                    .collect(),
                scene: self.scene,
            }),
            _ => None,
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        match self.mode {
            Mode::Curvature => {
                ui.horizontal_wrapped(|ui| {
                    for (mode, label) in [
                        (CurvatureMode::Mean, "Mean"),
                        (CurvatureMode::Gauss, "Gauss"),
                        (CurvatureMode::MinRadius, "Smallest radius"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut self.curvature, mode, label)
                            .changed();
                    }
                });
                changed |= ui
                    .checkbox(&mut self.tool_limit, "Check tool radius")
                    .changed();
                if self.tool_limit {
                    ui.horizontal(|ui| {
                        ui.label("Tool radius");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.tool_radius)
                                    .speed(0.1)
                                    .range(0.001..=1e6)
                                    .suffix(" mm"),
                            )
                            .changed();
                    });
                }
            }
            Mode::Draft => {
                ui.label("Pull direction");
                ui.horizontal_wrapped(|ui| {
                    for (axis, v) in AXES {
                        changed |= ui
                            .selectable_value(&mut self.pull, v, format!("Pull {axis}"))
                            .changed();
                    }
                });
                ui.collapsing("Custom pull", |ui| {
                    changed |= direction(ui, &mut self.pull);
                });
                ui.horizontal(|ui| {
                    ui.label("Minimum draft");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.minimum)
                                .speed(0.1)
                                .range(0.0..=90.)
                                .suffix("°"),
                        )
                        .changed();
                });
            }
            Mode::Access => {
                ui.label("Access directions");
                ui.horizontal_wrapped(|ui| {
                    for ((axis, _), on) in AXES.iter().zip(&mut self.access) {
                        changed |= ui.checkbox(on, *axis).changed();
                    }
                });
                let mut custom = self.access_vector.is_some();
                if ui.checkbox(&mut custom, "Custom direction").changed() {
                    self.access_vector = custom.then_some([1., 1., 1.]);
                    changed = true;
                }
                if let Some(dir) = &mut self.access_vector {
                    changed |= direction(ui, dir);
                }
                changed |= ui
                    .checkbox(&mut self.scene, "Include other parts")
                    .changed();
            }
            Mode::Zebra => {
                ui.label("Stripe count");
                changed |= ui.add(egui::Slider::new(&mut self.count, 1..=64)).changed();
            }
            Mode::Off => {}
        }
        changed
    }
}

fn direction(ui: &mut egui::Ui, vector: &mut [f64; 3]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (label, v) in ["X", "Y", "Z"].into_iter().zip(vector) {
            changed |= ui
                .add(
                    egui::DragValue::new(v)
                        .prefix(format!("{label} "))
                        .speed(0.05),
                )
                .changed();
        }
    });
    changed
}

impl Shell {
    fn invalidate_surface(&mut self) {
        self.surface.generation = self.surface.generation.wrapping_add(1);
        self.surface.reading = None;
        self.surface.error = None;
        self.surface.pending = self.surface.request().is_some();
        self.surface.centre_pending = self.surface.show_centre
            && self.surface.centre.is_none()
            && self.surface.centre_error.is_none();
        self.gpu.surface = None;
        self.gpu.zebra = if self.surface.mode == Mode::Zebra {
            self.surface.part.clone().map(|p| (p, self.surface.count))
        } else {
            None
        };
        self.dirty = true;
    }

    pub(super) fn sync_surface_source(&mut self) {
        if let Some(part) = &self.surface.part
            && (self.selected.as_deref() != Some(part)
                || self.hidden.contains(part)
                || self.surface.geometry.as_ptr() != Arc::as_ptr(&self.bodies))
        {
            self.surface.mode = Mode::Off;
            self.surface.show_centre = false;
            self.surface.part = None;
            self.surface.centre = None;
            self.surface.centre_error = None;
            self.invalidate_surface();
        }
    }

    fn sync_surface_pose(&mut self) {
        if !matches!(self.surface.mode, Mode::Draft | Mode::Access) {
            return;
        }
        let poses = self.move_poses();
        let Some(part) = self.surface.part.as_ref() else {
            return;
        };
        let check_all = self.surface.mode == Mode::Access && self.surface.scene;
        let changed = if check_all {
            *poses != self.surface.poses
        } else {
            poses.get(part) != self.surface.poses.get(part) || self.surface.poses.len() > 1
        };
        if changed {
            self.surface.poses = if check_all {
                poses.into_owned()
            } else {
                poses
                    .get(part)
                    .map(|&m| (part.clone(), m))
                    .into_iter()
                    .collect()
            };
            self.invalidate_surface();
        }
    }

    pub(super) fn surface_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Analysis", |ui| {
            let available = self
                .selected
                .as_ref()
                .is_some_and(|id| !self.hidden.contains(id));
            if !available {
                ui.weak("Select a visible part.");
            }
            ui.add_enabled_ui(available, |ui| {
                for mode in [Mode::Curvature, Mode::Draft, Mode::Access, Mode::Zebra] {
                    if ui
                        .selectable_label(self.surface.mode == mode, mode.name())
                        .clicked()
                    {
                        self.surface.part = self.selected.clone();
                        self.surface.geometry = Arc::downgrade(&self.bodies);
                        self.surface.mode = mode;
                        self.invalidate_surface();
                        ui.close();
                    }
                }
                ui.separator();
                if ui
                    .checkbox(&mut self.surface.show_centre, "Centre of mass")
                    .changed()
                {
                    self.surface.part = self.selected.clone();
                    self.surface.geometry = Arc::downgrade(&self.bodies);
                    self.surface.centre_error = None;
                    self.surface.centre_pending =
                        self.surface.show_centre && self.surface.centre.is_none();
                    self.dirty = true;
                    ui.close();
                }
            });
        });
    }

    pub(super) fn poll_surface(&mut self, ctx: &egui::Context) {
        self.sync_surface_source();
        self.sync_surface_pose();
        if let Some(job) = &self.surface.job {
            let result = match job.receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The surface analysis worker stopped.".into())
                }
            };
            if job.generation == self.surface.generation {
                match result {
                    Ok(Output::Surface(paint, value)) => {
                        self.gpu.surface = Some(paint);
                        self.surface.reading = Some(value);
                    }
                    Ok(Output::Centre(p)) => self.surface.centre = Some(p),
                    Err(message) => {
                        if job.centre {
                            self.surface.centre_error = Some(message);
                        } else {
                            self.surface.error = Some(message);
                        }
                    }
                }
                self.dirty = true;
            }
            self.surface.job = None;
        }
        let task = if std::mem::take(&mut self.surface.pending) {
            self.surface.request().map(Task::Surface)
        } else if std::mem::take(&mut self.surface.centre_pending) {
            Some(Task::Centre)
        } else {
            None
        };
        let (Some(task), Some(id)) = (task, self.surface.part.clone()) else {
            return;
        };
        let poses = self.surface.poses.clone();
        let is_centre = matches!(task, Task::Centre);
        let bodies = Arc::clone(&self.bodies);
        let device = self.gpu.device.clone();
        let ctx = ctx.clone();
        let (tx, receiver) = mpsc::channel();
        self.surface.job = Some(Job {
            generation: self.surface.generation,
            centre: is_centre,
            receiver,
        });
        std::thread::spawn(move || {
            let result = match task {
                Task::Surface(request) => {
                    faber_wiring::surface_pose::analyse_posed(&bodies, &id, &request, &poses)
                        .and_then(|mut a| {
                            if let Some(object) = a.report.as_object_mut() {
                                for key in ["faces", "hidden_faces"] {
                                    object.remove(key);
                                }
                                if let Some(faces) = object
                                    .remove("tool_too_large_faces")
                                    .and_then(|v| v.as_array().map(Vec::len))
                                {
                                    object.insert("tool_face_count".into(), faces.into());
                                }
                            }
                            let paint = SurfacePaint::new(&device, id, &a.mesh.mesh, &a.colours)?;
                            Ok(Output::Surface(
                                paint,
                                Reading {
                                    range: a.range,
                                    report: a.report,
                                },
                            ))
                        })
                }
                Task::Centre => bodies
                    .iter()
                    .find(|b| b.id == id)
                    .ok_or_else(|| "Select a part.".to_owned())
                    .and_then(centre_of_mass)
                    .map(Output::Centre),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn surface_panel(&mut self, ui: &mut egui::Ui) -> bool {
        if self.surface.mode == Mode::Off && !self.surface.show_centre {
            return false;
        }
        ui.horizontal(|ui| {
            ui.heading(if self.surface.mode == Mode::Off {
                "Mass properties"
            } else {
                self.surface.mode.name()
            });
            if ui.button("Close").clicked() {
                self.surface.mode = Mode::Off;
                self.surface.show_centre = false;
                self.surface.part = None;
                self.surface.centre = None;
                self.surface.centre_error = None;
                self.invalidate_surface();
            }
        });
        if let Some(id) = &self.surface.part {
            ui.weak(self.data.parts.get(id).map_or(id.as_str(), |p| &p.name));
        }
        if self.surface.controls(ui) {
            self.invalidate_surface();
            ui.ctx().request_repaint();
        }
        if let Some(r) = &self.surface.reading {
            legend(ui, &self.surface, r);
        } else if let Some(error) = &self.surface.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        } else if self.surface.request().is_some() {
            ui.weak("Measuring surfaces…");
        }
        if self.surface.show_centre {
            if let Some(point) = self.surface.centre {
                ui.label("Mass centre (mm)");
                let [x, y, z] = self.posed_centre(point).to_array();
                ui.monospace(format!("{x:.2}, {y:.2}, {z:.2}"));
            } else if let Some(error) = &self.surface.centre_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            } else {
                ui.weak("Finding the centre of mass…");
            }
        }
        true
    }

    fn posed_centre(&self, point: [f64; 3]) -> glam::DVec3 {
        let p = glam::DVec3::from_array(point);
        self.surface
            .part
            .as_ref()
            .and_then(|id| self.move_poses().get(id).copied())
            .map_or(p, |pose| pose.as_dmat4().transform_point3(p))
    }

    pub(super) fn surface_marker(&self, ui: &egui::Ui, rect: egui::Rect) {
        if !self.surface.show_centre {
            return;
        }
        let Some(p) = self.surface.centre.and_then(|p| {
            move_tool::screen_point(
                rect,
                self.camera.matrix(rect.aspect_ratio()),
                self.posed_centre(p),
            )
        }) else {
            return;
        };
        let ink = ui.painter().with_clip_rect(rect);
        ink.circle_filled(p, 8., Color32::WHITE);
        for sign in [1., -1.] {
            let mut wedge = vec![p];
            for step in 0..=8 {
                let t = std::f32::consts::FRAC_PI_2 * step as f32 / 8.;
                wedge.push(p + egui::vec2(t.cos(), t.sin()) * 7.5 * sign);
            }
            ink.add(egui::Shape::convex_polygon(
                wedge,
                Color32::BLACK,
                egui::Stroke::NONE,
            ));
        }
        ink.circle_stroke(p, 8., egui::Stroke::new(1.4, Color32::BLACK));
    }
}

fn swatch(ui: &mut egui::Ui, colour: [u8; 3], text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18., 9.), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 1., Color32::from_rgb(colour[0], colour[1], colour[2]));
        ui.small(text);
    });
}

fn legend(ui: &mut egui::Ui, panel: &SurfacePanel, reading: &Reading) {
    ui.separator();
    let report = &reading.report;
    match panel.mode {
        Mode::Curvature => {
            let title = match panel.curvature {
                CurvatureMode::Mean => "Mean curvature (1/mm)",
                CurvatureMode::Gauss => "Gaussian curvature (1/mm²)",
                CurvatureMode::MinRadius => "Smallest radius (mm)",
            };
            ui.label(title);
            let infinite =
                panel.curvature == CurvatureMode::MinRadius && reading.range == Some([0.; 2]);
            if infinite {
                let colour =
                    faber_core::surface::curvature_colour(f64::INFINITY, panel.curvature, [0.; 2]);
                swatch(ui, colour, "Infinite radius");
            } else if let Some(range @ [lo, hi]) = reading.range {
                let steps = if lo == hi { 1 } else { 5 };
                ui.horizontal_wrapped(|ui| {
                    for i in 0..steps {
                        let v = lo + (hi - lo) * f64::from(i) * 0.25;
                        let rgb = faber_core::surface::curvature_colour(v, panel.curvature, range);
                        ui.vertical(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(28., 8.), egui::Sense::hover());
                            ui.painter().rect_filled(
                                rect,
                                0.,
                                Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                            );
                            ui.small(format!("{v:.3}"));
                        });
                    }
                });
            }
            if panel.curvature == CurvatureMode::MinRadius && !infinite {
                ui.small("Flat faces have an infinite radius.");
            }
            if let Some(faces) = report["tool_face_count"].as_u64() {
                ui.label(format!("Tool exceeds the radius on {faces} faces."));
            }
        }
        Mode::Draft => {
            ui.label("Draft result");
            for (class, name, field) in [
                (DraftClass::Positive, "Positive", ""),
                (DraftClass::Negative, "Negative", ""),
                (DraftClass::Vertical, "Vertical", "vertical_mm2"),
                (
                    DraftClass::BelowMinimum,
                    "Below minimum",
                    "below_minimum_mm2",
                ),
                (DraftClass::Undercut, "Undercut", "undercut_mm2"),
            ] {
                let label = report
                    .get(field)
                    .and_then(|v| v.as_f64())
                    .map_or_else(|| name.to_owned(), |v| format!("{name} · {v:.2} mm²"));
                swatch(ui, draft_colour(class), &label);
            }
        }
        Mode::Access => {
            ui.label("Access result");
            for (on, label, key) in [
                (true, "Reachable", "reachable_mm2"),
                (false, "Hidden", "hidden_mm2"),
            ] {
                if let Some(v) = report[key].as_f64() {
                    swatch(ui, access_colour(on), &format!("{label} · {v:.2} mm²"));
                }
            }
        }
        _ => {}
    }
    if let Some(method) = report["method"].as_str() {
        ui.weak(method);
    }
    if let Some(triangles) = report["resolution"]["triangles"].as_u64() {
        ui.small(format!("{triangles} triangles"));
    }
    if let Some(limits) = report["limits"].as_array() {
        for text in limits.iter().filter_map(|v| v.as_str()) {
            ui.small(text);
        }
    }
}
