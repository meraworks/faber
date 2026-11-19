//! Check the placed parts and inspect the wall and orientation results.

use super::*;
use faber_model::Printer;
use faber_print::{Candidate, Finding, Kind, PrintReport};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

type WallPaint = BTreeMap<String, Vec<[u8; 3]>>;

#[derive(Clone, PartialEq)]
struct Query {
    path: PathBuf,
    revision: u64,
    part: Option<String>,
}

enum Update {
    Report(Result<(PrintReport, WallPaint), String>),
    Rank(Result<Vec<Candidate>, String>),
}

struct Worker {
    query: Query,
    result: mpsc::Receiver<Update>,
    stale: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct PrintPanel {
    pub(super) open: bool,
    part: Option<String>,
    current: Option<Query>,
    worker: Option<Worker>,
    started: bool,
    report: Option<PrintReport>,
    paint: WallPaint,
    overlay: bool,
    finding: Option<usize>,
    candidates: Option<Vec<Candidate>>,
    candidate: usize,
    error: Option<String>,
}

impl PrintPanel {
    pub(super) fn wall_paint(&self) -> Option<&WallPaint> {
        (self.open && self.overlay && self.report.is_some()).then_some(&self.paint)
    }
}

fn kind_label(kind: &Kind) -> &'static str {
    match kind {
        Kind::TooLarge => "Too large",
        Kind::ThinWall => "Thin wall",
        Kind::Overhang => "Overhang",
        Kind::Island => "Island",
        Kind::SmallContact => "Small bed contact",
        Kind::LongBridge => "Long bridge",
        Kind::TallAndNarrow => "Tall and narrow",
    }
}

fn check(
    bodies: &[Body],
    part: Option<&str>,
    printer: &Printer,
) -> Result<(PrintReport, WallPaint), String> {
    let selected: Vec<_> = bodies
        .iter()
        .filter(|b| part.is_none_or(|id| id == b.id))
        .collect();
    let meshes: Vec<_> = selected.iter().map(|b| &b.mesh).collect();
    let (report, thickness) = faber_print::check_with_walls(&meshes, printer, [0., 0., 1.])?;
    let mut paint = WallPaint::new();
    for (body, wall) in selected.into_iter().zip(thickness) {
        let thin = |w| faber_print::is_thin_wall(w, printer.min_wall_mm);
        if wall.iter().copied().any(thin) {
            let colours = wall
                .into_iter()
                .map(|w| if thin(w) { [236, 58, 45] } else { body.colour })
                .collect();
            paint.insert(body.id.clone(), colours);
        }
    }
    Ok((report, paint))
}

impl Shell {
    fn print_query(&self) -> Option<Query> {
        self.printing.open.then(|| Query {
            path: self.root.clone(),
            revision: self.data.revision,
            part: self.printing.part.clone(),
        })
    }

    pub(super) fn poll_print(&mut self, ctx: &egui::Context) {
        if self
            .printing
            .current
            .as_ref()
            .is_some_and(|q| q.path != self.root)
        {
            self.printing.open = false;
        }
        if self
            .printing
            .part
            .as_ref()
            .is_some_and(|id| !self.data.parts.contains_key(id))
        {
            self.printing.part = None;
        }
        let next = self.print_query();
        if self.printing.current != next {
            if let Some(worker) = &self.printing.worker {
                worker.stale.store(true, Ordering::Relaxed);
            }
            self.printing.current = next;
            self.printing.started = false;
            self.printing.report = None;
            self.printing.candidates = None;
            self.printing.paint.clear();
            self.printing.error = None;
            self.printing.candidate = 0;
            self.printing.finding = None;
            if self.printing.overlay {
                self.list = None;
                self.dirty = true;
            }
        }
        while let Some(worker) = &self.printing.worker {
            let key = &worker.query;
            let current = Some(key) == self.printing.current.as_ref()
                && !worker.stale.load(Ordering::Relaxed);
            match worker.result.try_recv() {
                Ok(update) if current => match update {
                    Update::Report(Ok((report, paint))) => {
                        self.printing.report = Some(report);
                        self.printing.paint = paint;
                        if self.printing.overlay {
                            self.list = None;
                            self.dirty = true;
                        }
                    }
                    Update::Rank(Ok(candidates)) => self.printing.candidates = Some(candidates),
                    Update::Report(Err(e)) | Update::Rank(Err(e)) => self.printing.error = Some(e),
                },
                Ok(_) => (),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if current
                        && self.printing.error.is_none()
                        && (self.printing.report.is_none()
                            || key.part.is_some() && self.printing.candidates.is_none())
                    {
                        self.printing.error = Some("The print check worker stopped.".into());
                    }
                    self.printing.worker = None;
                }
            }
        }
        if self.printing.worker.is_some() || self.printing.started || self.pending.is_some() {
            return;
        }
        let Some(query) = self.printing.current.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let bodies = self.bodies.clone();
        let printer = faber_wiring::print::printer(&self.data);
        let stale = Arc::new(AtomicBool::new(false));
        self.printing.worker = Some(Worker {
            query: query.clone(),
            result: rx,
            stale: stale.clone(),
        });
        self.printing.started = true;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let report = check(&bodies, query.part.as_deref(), &printer);
            let can_rank = report.is_ok();
            if tx.send(Update::Report(report)).is_err() {
                return;
            }
            ctx.request_repaint();
            if can_rank
                && !stale.load(Ordering::Relaxed)
                && let Some(part) = query.part
                && let Some(body) = bodies.iter().find(|b| b.id == part)
            {
                let ranked = faber_print::rank(&body.mesh, &printer, Default::default());
                let _ = tx.send(Update::Rank(ranked));
            }
            drop(tx);
            ctx.request_repaint();
        });
    }

    fn apply_print_orientation(&mut self, ctx: &egui::Context) {
        let Some(query) = self.print_query() else {
            return;
        };
        if self.pending.is_some() || Some(&query) != self.printing.current.as_ref() {
            return;
        }
        let Some(candidate) = self
            .printing
            .candidates
            .as_ref()
            .and_then(|rows| rows.get(self.printing.candidate))
            .cloned()
        else {
            return;
        };
        let Some(id) = query.part else { return };
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut project = faber_wiring::open(&query.path, false, None)?;
                if project.data.revision != query.revision {
                    return Err("The project changed. Check the part again.".into());
                }
                faber_wiring::print::apply_orientation(&mut project, &id, &candidate)?;
                Ok((project.data, project.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn focus_finding(&mut self, i: usize) {
        if let Some(row) = self
            .printing
            .report
            .as_ref()
            .and_then(|r| r.findings.get(i))
        {
            self.printing.finding = Some(i);
            self.camera.target = glam::DVec3::from(row.at).as_vec3();
            let span = row.area_mm2.max(0.).sqrt() as f32;
            self.camera.distance = self.camera.distance.min((span * 3.).max(12.));
            self.dirty = true;
        }
    }

    pub(super) fn print_marker(&self, ui: &egui::Ui, rect: egui::Rect) {
        if !self.printing.open {
            return;
        }
        let finding = self
            .printing
            .finding
            .and_then(|i| self.printing.report.as_ref()?.findings.get(i));
        if let Some(row) = finding
            && let Some(point) = move_tool::screen_point(
                rect,
                self.camera.matrix(rect.aspect_ratio()),
                glam::DVec3::from(row.at),
            )
        {
            let painter = ui.painter().with_clip_rect(rect);
            painter.circle_stroke(point, 8., egui::Stroke::new(3., Color32::from_gray(32)));
            painter.circle_stroke(point, 8., egui::Stroke::new(1.5, Color32::WHITE));
            painter.circle_filled(point, 2., Color32::WHITE);
        }
    }

    pub(super) fn print_panel(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.printing.open {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!self.bodies.is_empty(), egui::Button::new("Print check"))
                    .clicked()
                {
                    self.printing.open = true;
                    self.printing.overlay = false;
                    self.printing.part = self
                        .selected
                        .clone()
                        .or_else(|| (self.bodies.len() == 1).then(|| self.bodies[0].id.clone()));
                    ui.ctx().request_repaint();
                }
                if ui.button("Views").clicked() {
                    self.views.open = true;
                }
                if ui.button("Manufacture").clicked() {
                    self.open_machining();
                }
            });
            return self.printing.open;
        }
        let close = ui
            .horizontal(|ui| {
                ui.heading("Print check");
                ui.button("Close").clicked()
            })
            .inner;
        if close {
            self.printing.open = false;
            self.list = None;
            self.dirty = true;
            return false;
        }
        let printer = faber_wiring::print::printer(&self.data);
        let choice = ui
            .add_enabled_ui(self.pending.is_none(), |ui| {
                egui::ComboBox::from_label("Printer")
                    .selected_text(&printer.name)
                    .truncate()
                    .show_ui(ui, |ui| {
                        let mut choice = None;
                        for (_, profile) in faber_std::printers::profiles() {
                            if ui
                                .selectable_label(profile == printer, &profile.name)
                                .clicked()
                                && profile != printer
                            {
                                choice = Some(profile);
                            }
                        }
                        choice
                    })
                    .inner
                    .flatten()
            })
            .inner;
        if let Some(printer) = choice {
            self.save_sections(ui.ctx(), vec![faber_model::Edit::SetPrinter { printer }]);
        }
        let [x, y, z] = printer.bed_mm;
        ui.small(format!(
            "Bed {x} × {y} × {z} mm · Nozzle {} mm",
            printer.nozzle_mm
        ));
        let current = self.printing.part.clone();
        let name = current
            .as_ref()
            .and_then(|p| self.data.parts.get(p))
            .map_or("All parts", |p| p.name.as_str());
        egui::ComboBox::from_label("Check")
            .selected_text(name)
            .truncate()
            .show_ui(ui, |ui| {
                if self.data.parts.len() > 1 {
                    ui.selectable_value(&mut self.printing.part, None, "All parts");
                }
                for (id, part) in &self.data.parts {
                    ui.selectable_value(&mut self.printing.part, Some(id.clone()), &part.name);
                }
            });
        if self.printing.part != current {
            self.poll_print(ui.ctx());
        }
        if let Some(error) = &self.printing.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(report) = &self.printing.report else {
            if self.printing.error.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking part geometry…");
                });
            }
            return true;
        };
        ui.separator();
        if report.fits {
            ui.strong("Fits the printer");
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Exceeds the printer size");
        }
        let [x, y, z] = report.size_mm;
        ui.label(format!("Size {x:.1} × {y:.1} × {z:.1} mm"));
        egui::Grid::new("print_report")
            .num_columns(2)
            .show(ui, |ui| {
                for (name, value) in [
                    ("Height", format!("{:.2} mm", report.height_mm)),
                    ("Layers", report.layers.to_string()),
                    ("Bed contact", format!("{:.1} mm²", report.bed_contact_mm2)),
                    ("Overhang", format!("{:.1} mm²", report.overhang_mm2)),
                    (
                        "Estimated support",
                        format!("{:.2} cm³", report.support_mm3 / 1000.),
                    ),
                    ("Minimum wall", format!("{:.2} mm", report.min_wall_mm)),
                    ("Thin wall area", format!("{:.1} mm²", report.thin_wall_mm2)),
                    ("Islands", report.islands.to_string()),
                ] {
                    ui.label(name);
                    ui.label(value);
                    ui.end_row();
                }
            });
        if ui
            .checkbox(&mut self.printing.overlay, "Wall thickness overlay")
            .changed()
        {
            self.list = None;
            self.mesh_dirty = true;
        }
        if self.printing.overlay {
            ui.small(format!("Red marks walls below {} mm.", printer.min_wall_mm));
        }
        ui.separator();
        ui.strong("Findings");
        let select = findings(ui, &report.findings, self.printing.finding);
        for limit in &report.limits {
            ui.small(limit);
        }
        if let Some(i) = select {
            self.focus_finding(i);
        }
        ui.separator();
        ui.strong("Best orientation");
        if self.printing.part.is_none() {
            ui.small("Choose one part to compare its orientations.");
        } else if let Some(rows) = &self.printing.candidates {
            if rows.iter().all(|r| !r.fits) {
                ui.label("The part does not fit in any ranked orientation.");
            }
            for (i, row) in rows.iter().enumerate() {
                let [x, y, z] = row.rotation_deg;
                let label = format!("{} · {x:.0}°, {y:.0}°, {z:.0}°", i + 1);
                if ui
                    .selectable_label(i == self.printing.candidate, label)
                    .on_hover_text(format!(
                        "Support {:.2} cm³ · Height {:.1} mm · Bed contact {:.1} mm²",
                        row.support_mm3 / 1000.,
                        row.height_mm,
                        row.bed_contact_mm2
                    ))
                    .clicked()
                {
                    self.printing.candidate = i;
                }
            }
            if let Some(row) = rows.get(self.printing.candidate) {
                ui.small(format!(
                    "Estimated support {:.2} cm³ · Height {:.1} mm",
                    row.support_mm3 / 1000.,
                    row.height_mm
                ));
                let ready = row.fits && self.pending.is_none();
                if ui
                    .add_enabled(ready, egui::Button::new("Apply orientation"))
                    .clicked()
                {
                    self.apply_print_orientation(ui.ctx());
                }
            }
        } else if self.printing.error.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Ranking orientations…");
            });
        }
        true
    }
}

fn findings(ui: &mut egui::Ui, findings: &[Finding], selected: Option<usize>) -> Option<usize> {
    if findings.is_empty() {
        ui.label("No findings.");
        return None;
    }
    egui::ScrollArea::vertical()
        .id_salt("print_findings")
        .max_height(170.)
        .show(ui, |ui| {
            let mut clicked = None;
            for (i, row) in findings.iter().enumerate() {
                let label = format!("{} · {}", kind_label(&row.kind), row.message);
                if ui
                    .add(egui::Button::selectable(selected == Some(i), label).wrap())
                    .on_hover_text("Focus this region in the model")
                    .clicked()
                {
                    clicked = Some(i);
                }
            }
            clicked
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn upright_plate() -> (tempfile::TempDir, Harness<'static, Shell>) {
        let dir = tempfile::tempdir().unwrap();
        let mut p = faber_wiring::create(dir.path(), "Plate".into()).unwrap();
        let part: faber_model::Part<faber_model::Scalar> =
            serde_json::from_value(serde_json::json!({
                "name":"Upright plate","colour":[125,150,170],
                "placement":{"translation":[7,9,4],"rotation":[0,0,0]},
                "geometry":{"op":"box","size":[0.6,8,30]}
            }))
            .unwrap();
        p.put_part("plate".into(), part).unwrap();
        let root = dir.path().to_owned();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400., 1000.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, p.data, p.bodies, None));
        for _ in 0..3 {
            h.step();
        }
        h.get_by_label("Print check").click();
        h.step();
        h.step();
        (dir, h)
    }

    fn wait(h: &mut Harness<'_, Shell>, condition: impl Fn(&Harness<'_, Shell>) -> bool) {
        let end = Instant::now() + Duration::from_secs(30);
        while !condition(h) {
            assert!(
                Instant::now() < end,
                "Print check timed out. {:?}",
                h.state().error
            );
            h.step();
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    #[test]
    fn print_panel_lists_the_findings() {
        let (_dir, mut h) = upright_plate();
        wait(&mut h, |h| {
            h.query_by_label("Wall thickness overlay").is_some()
        });
        h.state_mut().camera.target += glam::Vec3::splat(25.);
        let current = h.state().camera.target;
        h.query_all_by_label_contains("Thin wall ·")
            .next()
            .unwrap()
            .click();
        h.step();
        assert_ne!(h.state().camera.target, current);
        let before = h
            .state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap();
        h.get_by_label("Wall thickness overlay").click();
        h.step();
        let after = h
            .state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap();
        let red = |bytes: &[u8]| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| {
                    p[0] > 140
                        && u16::from(p[0]) > 2 * u16::from(p[1])
                        && u16::from(p[0]) > 2 * u16::from(p[2])
                })
                .count()
        };
        assert!(red(&after) > red(&before) + 20, "Thin faces must turn red");
    }

    #[test]
    fn printer_selector_saves_the_project_profile() {
        let (dir, mut h) = upright_plate();
        let revision = h.state().data.revision;
        h.get_by_role_and_label(egui::Role::ComboBox, "Printer")
            .click();
        for _ in 0..3 {
            h.step();
        }
        h.get_by_label("Bambu Lab A1").click();
        h.step();
        wait(&mut h, |h| h.state().data.revision > revision);
        let saved = faber_wiring::read(dir.path()).unwrap().0;
        assert_eq!(saved.printer.unwrap().bed_mm, [256.; 3]);
        assert!(dir.path().join("manufacture.json").is_file());
    }

    #[test]
    fn apply_turns_the_part() {
        let (dir, mut h) = upright_plate();
        let before = h.state().data.clone();
        wait(&mut h, |h| h.query_by_label("Apply orientation").is_some());
        h.get_by_label("Apply orientation").scroll_to_me();
        for _ in 0..8 {
            h.step();
        }
        h.get_by_label("Apply orientation").click();
        h.step();
        wait(&mut h, |h| h.state().data.revision > before.revision);
        let (saved, bodies) = faber_wiring::read(dir.path()).unwrap();
        assert_eq!(saved.revision, before.revision + 1);
        assert_eq!(
            saved.parts["plate"].geometry,
            before.parts["plate"].geometry
        );
        assert_ne!(
            saved.parts["plate"].placement,
            before.parts["plate"].placement
        );
        assert!(bodies[0].bounds[0][2].abs() < 1e-6);
        assert!((bodies[0].bounds[1][2] - 0.6).abs() < 1e-5);
    }
}
