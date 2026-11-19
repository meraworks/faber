//! Set up a stress study and show its result in the model view.

mod kinds;
use super::*;
use faber_model::{
    FaceSelector, Scalar,
    study::{Fixture, FixtureKind, Study, StudyKind, StudyLoad},
};
use faber_wiring::study::{
    Analysis, StressReport, StudyField, quick_analysis_with_progress, quick_stress_with_progress,
};
use kinds::{Heat, Kind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

type StudyResult = (StressReport, StudyField);
enum Outcome {
    Static(Box<StudyResult>),
    Other(Analysis, Option<String>),
}

#[derive(Clone, Copy)]
enum Target {
    Fixture(usize),
    Force(usize),
    Pressure(usize),
    Heat(usize, Heat),
    Keep(usize),
}

struct Worker {
    stop: Arc<AtomicBool>,
    iteration: Arc<AtomicUsize>,
    latest: Arc<std::sync::Mutex<Option<(usize, StudyField)>>>,
    result: mpsc::Receiver<Result<Outcome, String>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(super) struct Paint {
    pub(super) bodies: Vec<Body>,
    pub(super) ratios: Vec<f32>,
    pub(super) part: String,
    points: Vec<glam::DVec3>,
}

#[derive(Default)]
pub(super) struct StudyPanel {
    form: Option<Study<Scalar>>,
    name: String,
    source: Option<(PathBuf, u64)>,
    stamp: Option<(SystemTime, u64)>,
    picking: Option<(Target, SelectFilter)>,
    worker: Option<Worker>,
    result: Option<StudyResult>,
    other: Option<Arc<Analysis>>,
    save_error: Option<String>,
    preview_iteration: Option<usize>,
    stopped: bool,
    mode: usize,
    animate: bool,
    animation_time: f64,
    thermal_stress: bool,
    error: Option<String>,
    pub(super) paint: Option<Paint>,
    deformed: bool,
    scale: f64,
}

fn new_study(part: String) -> Study<Scalar> {
    Study {
        kind: faber_model::study::StudyKind::Static,
        part,
        fixtures: vec![],
        loads: vec![],
        material: None,
        resolution_mm: None,
        print_direction: None,
        note: String::new(),
    }
}

fn scalar(ui: &mut egui::Ui, label: &str, value: &mut Scalar) {
    let mut text = match value {
        Scalar::Number(n) => n.to_string(),
        Scalar::Expression(e) => e.clone(),
    };
    let label = ui.label(label);
    let response = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(52.))
        .labelled_by(label.id);
    if response.changed() {
        *value = Scalar::Expression(text.clone());
    }
    if response.lost_focus()
        && let Ok(number) = text.parse::<f64>()
        && number.is_finite()
    {
        *value = Scalar::Number(number);
    }
}

fn vector(ui: &mut egui::Ui, name: &str, values: &mut [Scalar; 3]) {
    egui::Grid::new(name).num_columns(2).show(ui, |ui| {
        for (key, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            scalar(ui, key, value);
            ui.end_row();
        }
    });
}

impl Shell {
    fn clear_study_result(&mut self) {
        self.study.result = None;
        self.study.other = None;
        self.study.save_error = None;
        self.study.preview_iteration = None;
        self.study.stopped = false;
        self.study.animate = false;
        self.study.mode = 0;
        self.study.thermal_stress = false;
        if self.study.paint.take().is_some() {
            self.list = None;
            self.mesh_dirty = true;
            self.hover_pos = None;
        }
        self.study.error = None;
    }

    fn end_study_pick(&mut self) {
        if let Some((_, previous)) = self.study.picking.take() {
            self.set_filter(previous);
        }
    }

    fn start_study_pick(&mut self, target: Target) {
        self.end_study_pick();
        self.clear_study_result();
        let previous = self.select_filter;
        self.set_filter(SelectFilter::Face);
        self.clear_entity_picks();
        self.study.picking = Some((target, previous));
        self.study.error = None;
    }

    fn use_study_face(&mut self) {
        let Some((target, _)) = self.study.picking else {
            return;
        };
        let Some(Pick::Face { part, face }) = self.picks.last() else {
            return;
        };
        let Some(form) = self.study.form.as_ref() else {
            return;
        };
        if part != &form.part {
            self.study.error = Some(format!("Pick a face on {}.", form.part));
            return;
        }
        let Some(info) = self.pick_info_result(part) else {
            return;
        };
        let choice = info
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|topo| faber_core::pick_selector::face_selector(&topo.faces, &[*face]));
        match choice {
            Err(e) => self.study.error = Some(e),
            Ok(selector) => {
                let form = self.study.form.as_mut().unwrap();
                match target {
                    Target::Heat(i, kind) => {
                        if let StudyKind::Thermal { conditions, .. } = &mut form.kind {
                            if let Some(row) = conditions.get_mut(i) {
                                *kinds::condition_face(row) = selector;
                            } else {
                                conditions.push(kind.condition(selector));
                            }
                        }
                    }
                    Target::Keep(i) => {
                        if let StudyKind::Shape { keep, .. } = &mut form.kind {
                            if let Some(row) = keep.get_mut(i) {
                                *row = selector;
                            } else {
                                keep.push(selector);
                            }
                        }
                    }
                    Target::Fixture(i) => {
                        if let Some(row) = form.fixtures.get_mut(i) {
                            row.face = selector;
                        } else {
                            form.fixtures.push(Fixture {
                                kind: FixtureKind::Fixed,
                                face: selector,
                            });
                        }
                    }
                    Target::Force(i) | Target::Pressure(i) => {
                        if let Some(
                            StudyLoad::Force { face, .. } | StudyLoad::Pressure { face, .. },
                        ) = form.loads.get_mut(i)
                        {
                            *face = selector;
                        } else if matches!(target, Target::Force(_)) {
                            form.loads.push(StudyLoad::Force {
                                face: selector,
                                newtons: [0., 0., -10.].map(Scalar::Number),
                            });
                        } else {
                            form.loads.push(StudyLoad::Pressure {
                                face: selector,
                                mpa: Scalar::Number(1.),
                            });
                        }
                    }
                }
                self.clear_study_result();
                self.end_study_pick();
            }
        }
    }

    fn run_study(&mut self, ctx: &egui::Context) {
        let Some(form) = self.study.form.clone() else {
            return;
        };
        self.clear_study_result();
        let root = self.root.clone();
        let revision = self.data.revision;
        let stop = Arc::new(AtomicBool::new(false));
        let iteration = Arc::new(AtomicUsize::new(0));
        let latest = Arc::new(std::sync::Mutex::new(None));
        let (tx, rx) = mpsc::channel();
        self.study.worker = Some(Worker {
            stop: stop.clone(),
            iteration: iteration.clone(),
            latest: latest.clone(),
            result: rx,
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let project = faber_wiring::read_snapshot(&root)?;
                if project.data.revision != revision {
                    return Err("The project changed. Run the study again.".into());
                }
                if stop.load(Ordering::Relaxed) {
                    return Err("The study stopped.".into());
                }
                if matches!(form.kind, StudyKind::Static) {
                    quick_stress_with_progress(&project, &form, None, |i, _| {
                        iteration.store(i, Ordering::Relaxed);
                        !stop.load(Ordering::Relaxed)
                    })
                    .map(|s| Outcome::Static(Box::new(s)))
                } else {
                    let save_error = matches!(form.kind, StudyKind::Shape { .. })
                        .then(|| {
                            faber_wiring::study::check_shape_destination(&project, &form.part).err()
                        })
                        .flatten();
                    quick_analysis_with_progress(&project, &form, None, |i, _, field| {
                        iteration.store(i, Ordering::Relaxed);
                        if stop.load(Ordering::Relaxed) {
                            return false;
                        }
                        if let Some(value) = field {
                            *latest.lock().unwrap() = Some((i, value.clone()));
                            ctx.request_repaint();
                        }
                        true
                    })
                    .map(|result| Outcome::Other(result, save_error))
                }
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn paint_study(&mut self) {
        let Some((report, field)) = &self.study.result else {
            return;
        };
        let Some(index) = self.bodies.iter().position(|b| b.id == report.part) else {
            return;
        };
        let scale = if self.study.deformed {
            self.study.scale
        } else {
            0.
        };
        match faber_wiring::study_image::display(
            &self.bodies[index],
            field,
            scale,
            report.max_von_mises_mpa as f32,
        ) {
            Ok((body, ratios)) => {
                let source = field
                    .surface
                    .as_ref()
                    .map_or(&self.bodies[index].mesh.vertices, |s| &s.mesh.vertices);
                let points = report
                    .hot_spots
                    .iter()
                    .take(5)
                    .map(|hot| {
                        let p = glam::DVec3::from(hot.point);
                        if scale == 0. {
                            return p;
                        }
                        let nearest = source.iter().enumerate().min_by(|(_, a), (_, b)| {
                            p.distance_squared(glam::DVec3::from(**a))
                                .total_cmp(&p.distance_squared(glam::DVec3::from(**b)))
                        });
                        nearest.map_or(p, |(j, before)| {
                            p + glam::DVec3::from(body.mesh.vertices[j])
                                - glam::DVec3::from(*before)
                        })
                    })
                    .collect();
                self.show_study_body(index, body, ratios, points);
            }
            Err(e) => self.study.error = Some(e),
        }
    }

    fn show_study_body(
        &mut self,
        index: usize,
        body: Body,
        ratios: Vec<f32>,
        points: Vec<glam::DVec3>,
    ) {
        let updated = if self.press.is_none()
            && let Some(list) = &mut self.list
            && let Some(indices) = Arc::get_mut(&mut self.cut_indices)
            && let Some(entry) = indices.get_mut(index)
        {
            match list.update_field(&self.gpu, &body.id, &body.mesh, &ratios) {
                Ok(()) => {
                    *entry = faber_scene::SectionIndex::new(&body.mesh);
                    true
                }
                Err(e) => {
                    self.study.error = Some(e);
                    return;
                }
            }
        } else {
            false
        };
        let p = self.study.paint.get_or_insert_with(|| Paint {
            part: body.id.clone(),
            bodies: self.bodies.to_vec(),
            ratios: vec![],
            points: vec![],
        });
        p.bodies[index] = body;
        p.ratios = ratios;
        p.points = points;
        if !updated {
            self.list = None;
        }
        self.caps = None;
        self.cap_plane = None;
        self.mesh_dirty = true;
    }

    pub(super) fn poll_study(&mut self, ctx: &egui::Context) {
        if self.study.form.is_none() {
            return;
        }
        if self.study.source.as_ref().is_some_and(|(p, r)| {
            p != &self.root || *r != self.data.revision || self.study.stamp != self.modified
        }) {
            self.clear_study_result();
            self.end_study_pick();
            self.study.worker.take();
            if self
                .study
                .source
                .as_ref()
                .is_some_and(|(p, _)| p != &self.root)
                || self
                    .study
                    .form
                    .as_ref()
                    .is_some_and(|f| !self.data.parts.contains_key(&f.part))
            {
                self.study = Default::default();
                return;
            }
            self.study.source = Some((self.root.clone(), self.data.revision));
            self.study.stamp = self.modified;
        }
        self.use_study_face();
        self.animate_study(ctx);
        let event = self
            .study
            .worker
            .as_ref()
            .and_then(|w| w.latest.lock().unwrap().take());
        if let Some((i, field)) = event {
            self.paint_other_field(&field, 0., [0., 1.]);
            self.study.preview_iteration = Some(i);
        }
        let Some(worker) = &self.study.worker else {
            return;
        };
        match worker.result.try_recv() {
            Ok(done) => {
                if !worker.stop.load(Ordering::Relaxed) {
                    match done {
                        Ok(Outcome::Static(result)) => {
                            self.study.result = Some(*result);
                            self.paint_study();
                        }
                        Ok(Outcome::Other(result, save_error)) => {
                            self.study.save_error = save_error;
                            self.study.other = Some(Arc::new(result));
                            self.paint_analysis_field();
                        }
                        Err(e) => self.study.error = Some(e),
                    }
                } else {
                    self.study.stopped = self.study.preview_iteration.is_some();
                }
                self.study.worker = None;
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(70)),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.study.worker = None;
                self.study.error = Some("The study worker stopped.".into());
            }
        }
    }

    pub(super) fn study_markers(&self, ui: &egui::Ui, rect: egui::Rect) {
        let Some((report, _)) = &self.study.result else {
            return;
        };
        let Some(paint) = &self.study.paint else {
            return;
        };
        if self.hidden.contains(&report.part) || self.press.is_some() {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let pose = self.display_pose(&report.part);
        for (i, &point) in paint.points.iter().enumerate() {
            let placed = pose.as_dmat4().transform_point3(point);
            if let Some(p) = move_tool::screen_point(rect, matrix, placed) {
                painter.circle_filled(p, 7., Color32::from_rgb(33, 38, 43));
                painter.circle_stroke(p, 7., egui::Stroke::new(1., Color32::WHITE));
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    egui::FontId::monospace(10.),
                    Color32::WHITE,
                );
            }
        }
    }
}

fn face_name(face: &FaceSelector<Scalar>) -> String {
    let triple = |v: &[Scalar; 3]| {
        v.iter()
            .map(|s| match s {
                Scalar::Number(n) => format!("{n:.2}"),
                Scalar::Expression(e) => e.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(p) = &face.near_centre {
        format!("Centre ({}) mm", triple(p))
    } else if let Some(n) = &face.normal {
        format!("Normal ({})", triple(n))
    } else if let Some(name) = &face.created_by {
        format!("Face of {name}")
    } else {
        format!("Matched faces: {}", face.expect)
    }
}

fn fixture_rows(ui: &mut egui::Ui, form: &mut Study<Scalar>, pick: &mut Option<Target>) {
    let mut remove = None;
    for (i, row) in form.fixtures.iter_mut().enumerate() {
        ui.push_id(("fixture", i), |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("fixture_kind")
                    .selected_text(match row.kind {
                        FixtureKind::Fixed => "Fixed",
                        FixtureKind::Slide => "Slide",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut row.kind, FixtureKind::Fixed, "Fixed");
                        ui.selectable_value(&mut row.kind, FixtureKind::Slide, "Slide");
                    });
                if ui.button("Pick face").clicked() {
                    *pick = Some(Target::Fixture(i));
                }
                if ui
                    .small_button("×")
                    .on_hover_text("Remove fixture")
                    .clicked()
                {
                    remove = Some(i);
                }
            });
            ui.small(face_name(&row.face));
        });
    }
    if let Some(i) = remove {
        form.fixtures.remove(i);
    }
    if ui.button("Add fixture").clicked() {
        *pick = Some(Target::Fixture(form.fixtures.len()));
    }
}

fn load_rows(ui: &mut egui::Ui, form: &mut Study<Scalar>, pick: &mut Option<Target>) {
    let mut remove = None;
    for (i, row) in form.loads.iter_mut().enumerate() {
        ui.push_id(("load", i), |ui| {
            ui.horizontal(|ui| {
                let (name, target) = match row {
                    StudyLoad::Force { .. } => ("Force (N)", Some(Target::Force(i))),
                    StudyLoad::Pressure { .. } => ("Pressure (MPa)", Some(Target::Pressure(i))),
                    StudyLoad::Gravity { .. } => ("Gravity direction", None),
                };
                ui.label(name);
                if target.is_some() && ui.button("Pick face").clicked() {
                    *pick = target;
                }
                if ui.small_button("×").on_hover_text("Remove load").clicked() {
                    remove = Some(i);
                }
            });
            match row {
                StudyLoad::Force { face, newtons } => {
                    ui.small(face_name(face));
                    vector(ui, "force", newtons);
                }
                StudyLoad::Pressure { face, mpa } => {
                    ui.small(face_name(face));
                    ui.horizontal(|ui| scalar(ui, "Pressure (MPa)", mpa));
                }
                StudyLoad::Gravity { direction } => vector(ui, "direction", direction),
            }
        });
    }
    if let Some(i) = remove {
        form.loads.remove(i);
    }
    ui.menu_button("Add load", |ui| {
        if ui.button("Force").clicked() {
            *pick = Some(Target::Force(form.loads.len()));
            ui.close();
        }
        if ui.button("Pressure").clicked() {
            *pick = Some(Target::Pressure(form.loads.len()));
            ui.close();
        }
        if ui.button("Gravity").clicked() {
            form.loads.push(StudyLoad::Gravity {
                direction: [0., 0., -1.].map(Scalar::Number),
            });
            ui.close();
        }
    });
}

fn study_controls(
    ui: &mut egui::Ui,
    form: &mut Study<Scalar>,
    data: &ProjectData,
    pick: &mut Option<Target>,
) {
    let mut chosen = Kind::of(&form.kind);
    let old_kind = chosen;
    egui::ComboBox::from_label("Study kind")
        .selected_text(chosen.name())
        .truncate()
        .show_ui(ui, |ui| {
            for kind in [Kind::Static, Kind::Modal, Kind::Thermal, Kind::Shape] {
                ui.selectable_value(&mut chosen, kind, kind.name());
            }
        });
    if chosen != old_kind {
        form.kind = chosen.defaults();
    }
    let old_part = form.part.clone();
    egui::ComboBox::from_label("Part")
        .selected_text(&form.part)
        .truncate()
        .show_ui(ui, |ui| {
            for id in data.parts.keys() {
                ui.selectable_value(&mut form.part, id.clone(), id);
            }
        });
    if form.part != old_part {
        form.fixtures.clear();
        form.loads.clear();
        match &mut form.kind {
            StudyKind::Thermal { conditions, .. } => conditions.clear(),
            StudyKind::Shape { keep, symmetry, .. } => {
                keep.clear();
                symmetry.clear();
            }
            _ => {}
        }
    }
    let part_material = data
        .parts
        .get(&form.part)
        .and_then(|p| p.material.as_deref());
    let use_part = part_material.map_or_else(
        || "Part material (unassigned)".to_owned(),
        |id| format!("Part material ({id})"),
    );
    egui::ComboBox::from_label("Material")
        .selected_text(form.material.as_deref().unwrap_or(&use_part))
        .truncate()
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut form.material, None, use_part);
            for id in faber_core::materials::ids(data) {
                if let Ok(row) = faber_core::materials::get(data, id) {
                    ui.selectable_value(&mut form.material, Some(id.to_owned()), row.name);
                }
            }
        });
    kinds::setup(ui, form, data, pick);
    if !matches!(form.kind, StudyKind::Thermal { stress: false, .. }) {
        ui.separator();
        ui.strong("Fixtures");
        fixture_rows(ui, form, pick);
    }
    if matches!(form.kind, StudyKind::Static | StudyKind::Shape { .. }) {
        ui.separator();
        ui.strong("Loads");
        load_rows(ui, form, pick);
    }
    ui.separator();
    let mut automatic = form.resolution_mm.is_none();
    if ui
        .checkbox(&mut automatic, "Automatic resolution")
        .changed()
    {
        form.resolution_mm = (!automatic).then_some(Scalar::Number(2.));
    }
    if let Some(size) = &mut form.resolution_mm {
        ui.horizontal(|ui| scalar(ui, "Cell size (mm)", size));
    }
    if !matches!(form.kind, StudyKind::Static) {
        return;
    }
    let mut layers = form.print_direction.is_some();
    if ui.checkbox(&mut layers, "Use print direction").changed() {
        form.print_direction = layers.then_some([0., 0., 1.].map(Scalar::Number));
    }
    if let Some(v) = &mut form.print_direction {
        vector(ui, "Layers", v);
    }
}

impl Shell {
    pub(super) fn study_panel(&mut self, ui: &mut egui::Ui) -> bool {
        if self.study.form.is_none() {
            if ui
                .add_enabled(
                    !self.data.parts.is_empty(),
                    egui::Button::new("Stress study"),
                )
                .clicked()
            {
                let part = self
                    .selected
                    .clone()
                    .or_else(|| self.data.parts.keys().next().cloned())
                    .unwrap();
                let (name, form) = self
                    .data
                    .studies
                    .iter()
                    .next()
                    .map(|(id, s)| (id.clone(), s.clone()))
                    .unwrap_or_else(|| ("stress".into(), new_study(part)));
                self.study.name = name;
                self.study.form = Some(form);
                self.study.source = Some((self.root.clone(), self.data.revision));
                self.study.stamp = self.modified;
                self.study.scale = 1.;
            }
            return false;
        }
        let close = ui
            .horizontal(|ui| {
                ui.heading("Simulation study");
                ui.button("Close").clicked()
            })
            .inner;
        if close {
            self.end_study_pick();
            self.clear_study_result();
            self.study = Default::default();
            return false;
        }
        let mut pick = None;
        let idle = self.study.worker.is_none();
        let mut choice = None;
        ui.add_enabled_ui(idle, |ui| {
            egui::ComboBox::from_label("Study")
                .selected_text(&self.study.name)
                .show_ui(ui, |ui| {
                    for name in self.data.studies.keys() {
                        if ui
                            .selectable_label(name == &self.study.name, name)
                            .clicked()
                        {
                            choice = Some(Some(name.clone()));
                        }
                    }
                    if ui.selectable_label(false, "New study").clicked() {
                        choice = Some(None);
                    }
                });
        });
        if let Some(choice) = choice {
            self.end_study_pick();
            self.clear_study_result();
            if let Some(name) = choice {
                self.study.form = self.data.studies.get(&name).cloned();
                self.study.name = name;
            } else {
                let part = self.study.form.as_ref().unwrap().part.clone();
                self.study.form = Some(new_study(part));
                self.study.name = "stress".into();
            }
        }
        if let Some((target, _)) = self.study.picking {
            let what = match target {
                Target::Fixture(_) => "the fixture",
                Target::Heat(..) => "the heat condition",
                Target::Keep(_) => "the kept region",
                _ => "the load",
            };
            ui.label(format!("Pick a model face for {what}."));
            if ui.button("Cancel face pick").clicked() {
                self.end_study_pick();
            }
        }
        let form = self.study.form.as_mut().unwrap();
        let previous = form.clone();
        ui.add_enabled_ui(idle && self.study.picking.is_none(), |ui| {
            study_controls(ui, form, &self.data, &mut pick)
        });
        if *form != previous {
            self.clear_study_result();
        }
        if let Some(target) = pick {
            self.start_study_pick(target);
        }
        ui.separator();
        if let Some(worker) = &self.study.worker {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Iteration {}",
                    worker.iteration.load(Ordering::Relaxed)
                ));
                if ui
                    .add_enabled(
                        !worker.stop.load(Ordering::Relaxed),
                        egui::Button::new("Stop"),
                    )
                    .clicked()
                {
                    worker.stop.store(true, Ordering::Relaxed);
                }
            });
        } else {
            let form = self.study.form.as_ref().unwrap();
            let missing = match &form.kind {
                StudyKind::Thermal { conditions, .. } if conditions.is_empty() => {
                    Some("Add a heat condition.")
                }
                StudyKind::Thermal { conditions, .. }
                    if conditions.iter().all(|c| {
                        matches!(c, faber_model::study::ThermalCondition::Power { .. })
                    }) =>
                {
                    Some("Add a fixed temperature or convection.")
                }
                StudyKind::Thermal { stress: true, .. } if form.fixtures.is_empty() => {
                    Some("Add a fixture to calculate thermal stress.")
                }
                StudyKind::Static | StudyKind::Shape { .. } if form.fixtures.is_empty() => {
                    Some("Add a fixture.")
                }
                StudyKind::Static | StudyKind::Shape { .. } if form.loads.is_empty() => {
                    Some("Add a load.")
                }
                _ => None,
            };
            if let Some(reason) = missing {
                ui.small(reason);
            }
            let ready = self.study.picking.is_none() && self.pending.is_none() && missing.is_none();
            if ui
                .add_enabled(ready, egui::Button::new("Run study"))
                .clicked()
            {
                self.run_study(ui.ctx());
            }
            ui.collapsing("Save study", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.study.name);
                });
                if ui
                    .add_enabled(
                        !self.study.name.trim().is_empty() && self.pending.is_none(),
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    self.save_sections(
                        ui.ctx(),
                        vec![faber_model::Edit::SetStudy {
                            id: self.study.name.trim().into(),
                            study: self.study.form.as_ref().unwrap().clone(),
                        }],
                    );
                }
            });
        }
        if let Some(e) = &self.study.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some((report, _)) = &self.study.result {
            ui.separator();
            ui.strong("Results");
            ui.small(format!(
                "{} cells · {} iterations",
                report.cells, report.iterations
            ));
            ui.label(format!(
                "Largest stress: {:.3} MPa",
                report.max_von_mises_mpa
            ));
            ui.label(format!(
                "Largest displacement: {:.3} mm",
                report.max_displacement_mm
            ));
            let factor = format!("Least safety factor: {:.3}", report.min_safety_factor);
            if report.min_safety_factor < 1. {
                ui.colored_label(Color32::LIGHT_RED, factor);
            } else {
                ui.label(factor);
            }
            if let Some(value) = report.layer_safety_factor {
                let text = format!("Layer safety factor: {value:.3}");
                if value < 1. {
                    ui.colored_label(Color32::LIGHT_RED, text);
                } else {
                    ui.label(text);
                }
            }
            ui.small("Von Mises stress (MPa)");
            ui.horizontal_wrapped(|ui| {
                for step in 0..5 {
                    let max = report.max_von_mises_mpa as f32;
                    let value = max * step as f32 * 0.25;
                    let [r, g, b] = faber_wiring::study_image::colour(value, max);
                    ui.vertical(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(29., 7.), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 0., Color32::from_rgb(r, g, b));
                        ui.small(format!("{value:.2}"));
                    });
                }
            });
            let mut changed = ui
                .checkbox(&mut self.study.deformed, "Deformed shape")
                .changed();
            if self.study.deformed {
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.study.scale)
                                .range(0. ..=1e6)
                                .speed(0.2),
                        )
                        .changed();
                });
            }
            for limit in &report.limits {
                ui.small(limit);
            }
            if changed {
                self.paint_study();
            }
        }
        self.study_kind_result(ui);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn bracket() -> (tempfile::TempDir, Harness<'static, Shell>) {
        let dir = tempfile::tempdir().unwrap();
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/shelf-bracket-study");
        std::fs::create_dir(dir.path().join("parts")).unwrap();
        for file in ["faber.json", "parts/bracket.json", "analysis.json"] {
            std::fs::copy(source.join(file), dir.path().join(file)).unwrap();
        }
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let root = dir.path().to_owned();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400., 1000.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..3 {
            h.step();
        }
        h.get_by_label("Stress study").click();
        h.step();
        h.step();
        h.state_mut().study.form.as_mut().unwrap().resolution_mm =
            Some(faber_model::Scalar::Number(3.));
        h.step();
        h.step();
        (dir, h)
    }

    fn run(h: &mut Harness<'_, Shell>) {
        h.get_by_label("Run study").click();
        h.step();
        assert!(
            h.state().study.worker.is_some(),
            "The run button must start a study"
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        while h.state().study.worker.is_some() {
            assert!(Instant::now() < deadline, "The study did not finish");
            std::thread::sleep(Duration::from_millis(8));
            h.step();
        }
        assert!(
            h.state().study.error.is_none(),
            "{:?}",
            h.state().study.error
        );
        assert!(
            h.state().study.result.is_some(),
            "The worker must return a stress field"
        );
        h.step();
    }

    #[test]
    fn study_panel_runs_and_shows_the_factor() {
        let (_dir, mut h) = bracket();
        run(&mut h);
        assert!(h.query_by_label_contains("Least safety factor").is_some());
        assert!(h.query_by_label("Deformed shape").is_some());
        assert!(h.state().study.result.is_some());
        let pixels = h
            .state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap();
        let violet = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 80 && p[2] > 80 && p[1].saturating_add(24) < p[0].min(p[2]))
            .count();
        assert_eq!(
            violet, 0,
            "The viewer must show the colours of the stress legend."
        );
        let original = h.state().bodies[0].mesh.vertices.clone();
        h.get_by_label("Deformed shape").scroll_to_me();
        for _ in 0..8 {
            h.step();
        }
        h.get_by_label("Deformed shape").click();
        h.step();
        assert_ne!(
            h.state().study.paint.as_ref().unwrap().bodies[0]
                .mesh
                .vertices,
            original
        );
        assert_eq!(h.state().bodies[0].mesh.vertices, original);
    }

    #[test]
    fn pick_face_adds_a_fixture_to_the_study() {
        use super::super::controls_fixture::{centre, click, two_blocks};
        let (_dir, mut h) = two_blocks();
        h.get_by_label("Stress study").click();
        h.step();
        h.step();
        h.get_by_label("Add fixture").click();
        h.step();
        assert!(matches!(h.state().select_filter, SelectFilter::Face));
        let on_part = centre(&h, "left");
        click(
            &mut h,
            on_part,
            egui::PointerButton::Primary,
            egui::Modifiers::NONE,
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.state().study.picking.is_some() {
            assert!(
                Instant::now() < deadline,
                "The face did not enter the study"
            );
            h.step();
            std::thread::sleep(Duration::from_millis(4));
        }
        let form = h.state().study.form.as_ref().unwrap();
        assert_eq!(form.fixtures.len(), 1);
        assert_eq!(form.fixtures[0].face.expect, 1);
        assert_eq!(h.state().select_filter, SelectFilter::Body);
    }

    #[test]
    fn stop_ends_the_worker() {
        let (_dir, mut h) = bracket();
        h.state_mut().study.form.as_mut().unwrap().resolution_mm =
            Some(faber_model::Scalar::Number(1.));
        h.step();
        h.get_by_label("Run study").click();
        h.step();
        h.step();
        h.get_by_label("Stop").click();
        h.step();
        let deadline = Instant::now() + Duration::from_secs(30);
        while h.state().study.worker.is_some() {
            assert!(Instant::now() < deadline);
            h.step();
            std::thread::sleep(Duration::from_millis(8));
        }
        assert!(h.state().study.result.is_none());
        assert!(h.state().study.error.is_none());
    }

    #[test]
    fn result_clears_after_a_geometry_change() {
        let (dir, mut h) = bracket();
        run(&mut h);
        let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
        let mut part = project.data.parts["bracket"].clone();
        part.placement.translation[0] = faber_model::Scalar::Number(4.);
        project.put_part("bracket".into(), part).unwrap();
        h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.state().data.revision != project.data.revision {
            assert!(
                Instant::now() < deadline,
                "The viewer did not reload the part"
            );
            h.step();
            std::thread::sleep(Duration::from_millis(8));
        }
        h.step();
        assert!(h.state().study.result.is_none());
        assert!(h.query_by_label("Deformed shape").is_none());
    }
}

#[cfg(test)]
mod study_kind_tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn click(h: &mut Harness<'_, Shell>, name: &str) {
        h.get_by_label(name).scroll_to_me();
        for _ in 0..6 {
            h.step();
        }
        h.get_by_label(name).click();
        h.step();
        h.step();
    }

    fn beam() -> (tempfile::TempDir, Harness<'static, Shell>) {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, None).unwrap();
        let part: faber_model::Part<Scalar> = serde_json::from_value(serde_json::json!({
            "name":"Test beam","colour":[90,100,150],"material":"steel-s235",
            "geometry":{"op":"history","output":"stock","features":[
                {"id":"stock","geometry":{"op":"box","size":[28,8,8]}},
                {"id":"middle","geometry":{"op":"plane_on_face","source":"stock","offset":-4,
                    "face":{"normal":[0,1,0],"expect":1,"tolerance":0.001}}}
            ]}
        }))
        .unwrap();
        project.put_part("beam".into(), part).unwrap();
        let study = serde_json::from_value(serde_json::json!({"part":"beam","resolution_mm":2,
            "fixtures":[{"kind":"fixed","face":{"normal":[-1,0,0],"tolerance":0.001,"expect":1}}],
            "loads":[{"kind":"force","face":{"normal":[1,0,0],"tolerance":0.001,"expect":1},"newtons":[0,0,-10]}]}))
        .unwrap();
        project
            .batch(vec![faber_model::Edit::SetStudy {
                id: "beam_test".into(),
                study,
            }])
            .unwrap();
        let cold: faber_model::study::Study<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
            "part":"beam","resolution_mm":2,"kind":"thermal","conditions":[
                {"kind":"temperature","face":{"normal":[-1,0,0],"tolerance":0.001,"expect":1},"celsius":-10},
                {"kind":"temperature","face":{"normal":[1,0,0],"tolerance":0.001,"expect":1},"celsius":30}
            ],"stress":false,"reference_c":20,"fixtures":[],"loads":[]
        })).unwrap();
        project
            .batch(vec![faber_model::Edit::SetStudy {
                id: "cold_beam".into(),
                study: cold,
            }])
            .unwrap();
        let root = dir.path().to_owned();
        let (data, bodies) = (project.data, project.bodies);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1150., 950.))
            .with_step_dt(1. / 30.)
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        h.step();
        h.step();
        click(&mut h, "Stress study");
        (dir, h)
    }

    fn choose(h: &mut Harness<'_, Shell>, kind: &str) {
        click(h, "Study kind");
        click(h, kind);
    }
    fn wait(h: &mut Harness<'_, Shell>, mut condition: impl FnMut(&Harness<'_, Shell>) -> bool) {
        let start = Instant::now();
        while !condition(h) {
            assert!(
                start.elapsed() < Duration::from_secs(45),
                "{:?}",
                h.state().study.error
            );
            h.step();
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    #[test]
    fn kind_selector_changes_the_form() {
        let (_d, mut h) = beam();
        choose(&mut h, "Frequency");
        h.get_by_label("Number of modes");
        assert!(h.query_by_label("Add load").is_none());
        choose(&mut h, "Thermal");
        h.get_by_label("Add heat condition");
        h.get_by_label("Thermal stress");
        choose(&mut h, "Shape optimisation");
        h.get_by_label("Target volume fraction");
        h.get_by_label("Add kept face");
        click(&mut h, "Symmetry · middle");
        assert!(
            matches!(&h.state().study.form.as_ref().unwrap().kind, StudyKind::Shape{symmetry,..} if symmetry==&["middle"])
        );
        choose(&mut h, "Static stress");
        h.get_by_label("Add load");
    }

    #[test]
    fn mode_list_shows_frequencies() {
        let (_d, mut h) = beam();
        choose(&mut h, "Frequency");
        click(&mut h, "Run study");
        wait(&mut h, |h| h.query_by_label("Animate mode").is_some());
        assert!(h.query_by_label_contains("Mode 2 ·").is_some());
        let camera = h.state().camera.matrix(1.);
        let original = h.state().bodies[0].mesh.vertices.clone();
        let frame = super::super::controls_fixture::canvas(&h);
        h.get_by_label_contains("Mode 2 ·").scroll_to_me();
        for _ in 0..6 {
            h.step();
        }
        h.get_by_label_contains("Mode 2 ·").click();
        h.step();
        click(&mut h, "Animate mode");
        let first = h.state().study.paint.as_ref().unwrap().bodies[0]
            .mesh
            .vertices
            .clone();
        let index = Arc::as_ptr(&h.state().cut_indices);
        let mode_row = h.get_by_label_contains("Mode 2 ·").rect();
        for _ in 0..8 {
            h.step();
            assert_eq!(
                Arc::as_ptr(&h.state().cut_indices),
                index,
                "Animating one part rebuilt the entire scene index."
            );
            assert_eq!(super::super::controls_fixture::canvas(&h), frame);
            assert_eq!(h.get_by_label_contains("Mode 2 ·").rect(), mode_row);
        }
        assert_ne!(
            h.state().study.paint.as_ref().unwrap().bodies[0]
                .mesh
                .vertices,
            first
        );
        assert_eq!(h.state().camera.matrix(1.), camera);
        assert_eq!(h.state().bodies[0].mesh.vertices, original);
    }

    #[test]
    fn thermal_study_paints_a_signed_temperature_range() {
        let (_d, mut h) = beam();
        click(&mut h, "Study");
        click(&mut h, "cold_beam");
        click(&mut h, "Run study");
        wait(&mut h, |h| {
            h.query_by_label("Surface temperature (°C)").is_some()
        });
        let p = h.state().study.paint.as_ref().unwrap();
        assert!(p.ratios.iter().any(|&v| v < 0.02));
        assert!(p.ratios.iter().any(|&v| v > 0.98));
        h.get_by_label_contains("-10.0");
        h.get_by_label_contains("30.0");
        click(&mut h, "Thermal stress");
        h.get_by_label("Add a fixture to calculate thermal stress.");
        assert!(h.get_by_label("Run study").accesskit_node().is_disabled());
    }

    #[test]
    fn shape_preview_updates_and_stop_keeps_the_last_result() {
        let (_d, mut h) = beam();
        choose(&mut h, "Shape optimisation");
        click(&mut h, "Run study");
        let initial_camera = h.state().camera.matrix(1.);
        let canvas = super::super::controls_fixture::canvas(&h);
        let from = canvas.center();
        let to = from + egui::vec2(45., -12.);
        h.hover_at(from);
        h.step();
        h.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Middle,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        h.hover_at(to);
        h.step();
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Middle,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        assert_ne!(h.state().camera.matrix(1.), initial_camera);
        wait(&mut h, |h| {
            h.state().study.paint.is_some() && h.state().study.worker.is_some()
        });
        let camera = h.state().camera.matrix(1.);
        h.get_by_label("Stop").click();
        h.step();
        wait(&mut h, |h| h.state().study.worker.is_none());
        let shown = h.state().study.paint.as_ref().unwrap().bodies[0]
            .mesh
            .vertices
            .clone();
        h.get_by_label("Stopped at the last preview");
        assert!(
            h.get_by_label("Save as part")
                .accesskit_node()
                .is_disabled()
        );
        assert!(!h.state().data.parts.contains_key("beam-shape"));
        let stopped_pixels = h
            .state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap();
        for _ in 0..5 {
            h.step();
        }
        let after_idle = h
            .state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap();
        assert!(
            stopped_pixels == after_idle,
            "The stopped preview pixels changed while the camera was idle."
        );
        assert_eq!(
            h.state().study.paint.as_ref().unwrap().bodies[0]
                .mesh
                .vertices,
            shown
        );
        assert_eq!(h.state().camera.matrix(1.), camera);
        click(&mut h, "Run study");
        wait(&mut h, |h| h.state().study.worker.is_none());
        click(&mut h, "Save as part");
        wait(&mut h, |h| h.state().data.parts.contains_key("beam-shape"));
    }
}
