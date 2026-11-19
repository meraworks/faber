//! Edit the sheet rule and its straight flanges. Show the developed cutting outline.
use super::*;
use faber_model::{Edit, Feature, Flange, Geometry, Part, Scalar};
use faber_wiring::sheet::{self as metal, FlangeEdge, FlatPattern, recipe};
use std::sync::Arc;
mod form;
mod pattern;
use form::{FlangeForm, RuleForm};

enum Mode {
    View,
    New(RuleForm),
    Rule(RuleForm),
    Flange(FlangeForm),
}
enum Action {
    None,
    Close,
    Cancel,
    Save,
    Rule,
    Add,
    Edit(usize),
    Export,
    Browse,
}
struct Read {
    pattern: FlatPattern,
    edges: Vec<FlangeEdge>,
    stamp: Option<(SystemTime, u64)>,
}
type Job<T> = mpsc::Receiver<Result<T, String>>;

pub(super) struct Panel {
    part: String,
    mode: Mode,
    previous_filter: Option<SelectFilter>,
    expected: Option<ProjectData>,
    saving: bool,
    flat: bool,
    bends: bool,
    path: String,
    revision: Option<(u64, Option<(SystemTime, u64)>)>,
    read: Option<Job<Read>>,
    #[cfg(test)]
    pub(super) read_gate: Option<mpsc::Receiver<()>>,
    result: Option<Arc<Read>>,
    current: bool,
    edit_current: bool,
    export: Option<Job<String>>,
    message: Option<String>,
    zoom: f32,
    pan: Vec2,
}

fn receive<T>(slot: &mut Option<Job<T>>) -> Option<Result<T, String>> {
    let r = match slot.as_ref()?.try_recv() {
        Ok(x) => x,
        Err(mpsc::TryRecvError::Empty) => return None,
        Err(mpsc::TryRecvError::Disconnected) => Err("The sheet worker stopped.".into()),
    };
    *slot = None;
    Some(r)
}
pub(super) fn source_name(root: &std::path::Path) -> Option<String> {
    let source = faber_wiring::design_source();
    source
        .has_source(root)
        .then(|| source.file_name().to_owned())
}
impl Panel {
    fn read(&mut self, app: &Shell, ctx: &egui::Context) {
        let key = (app.data.revision, app.modified);
        if self.revision != Some(key) {
            self.revision = Some(key);
            self.current = false;
            self.edit_current = self
                .expected
                .as_ref()
                .is_none_or(|snapshot| snapshot == &app.data);
            self.message = None;
            self.export = None;
            let root = app.root.clone();
            let data = app.data.clone();
            let id = self.part.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.read = Some(rx);
            #[cfg(test)]
            let gate = self.read_gate.take();
            std::thread::spawn(move || {
                let result = (|| {
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        gate.recv()
                            .map_err(|_| "The sheet worker stopped.".to_owned())?;
                    }
                    let stamp = faber_wiring::project_fingerprint(&root)?;
                    let (pattern, edges) = metal::inspect(&data, &id)?;
                    // A local edit can reach the viewer before the file watcher reads its stamp.
                    // Display the recipe now. Enable export when the file and viewer agree.
                    let stamp = (Some(stamp) == key.1
                        && faber_wiring::project_fingerprint(&root)? == stamp)
                        .then_some(stamp);
                    Ok(Read {
                        pattern,
                        edges,
                        stamp,
                    })
                })();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        if let Some(result) = receive(&mut self.read) {
            match result {
                Ok(v) => {
                    self.result = Some(Arc::new(v));
                    self.current = true;
                }
                Err(e) => self.message = Some(e),
            }
        }
        if let Some(result) = receive(&mut self.export) {
            self.message = Some(result.unwrap_or_else(|e| e));
        }
    }
    fn make_edit(&self, data: &ProjectData) -> Result<Edit, String> {
        let id = self.part.clone();
        if let Mode::New(form) = &self.mode {
            if data.parts.contains_key(&id) {
                return Err("The sheet identifier is in use.".into());
            }
            let geometry = Geometry::SheetMetal {
                rule: form.rule(),
                base: form.base(),
                flanges: Vec::new(),
            };
            let part = Part {
                component: None,
                material: None,
                name: format!("Sheet {}", id.trim_start_matches("sheet_")),
                geometry: Geometry::History {
                    features: vec![Feature {
                        id: "fold".into(),
                        geometry,
                        suppressed: false,
                    }],
                    output: "fold".into(),
                },
                placement: Default::default(),
                colour: [144, 170, 185],
                group: String::new(),
                note: String::new(),
            };
            return Ok(Edit::PutPart { id, part });
        }
        let mut part = data.parts.get(&id).ok_or("The sheet was removed.")?.clone();
        faber_core::pick_selector::editable_part(&id, &part)?;
        let g = match &mut part.geometry {
            Geometry::History { features, output } => {
                &mut features
                    .iter_mut()
                    .find(|f| f.id == *output && !f.suppressed)
                    .ok_or("The sheet feature was removed.")?
                    .geometry
            }
            g => g,
        };
        let Geometry::SheetMetal { rule, flanges, .. } = g else {
            return Err("Select the sheet feature again.".into());
        };
        match &self.mode {
            Mode::Rule(f) => *rule = f.rule(),
            Mode::Flange(f) => f.apply(flanges)?,
            _ => return Err("Open a sheet feature to edit it.".into()),
        }
        Ok(Edit::PutPart { id, part })
    }
    fn restore_filter(&mut self, app: &mut Shell) {
        if let Some(old) = self.previous_filter.take() {
            app.set_filter(old);
        }
    }
}
impl Shell {
    pub(super) fn close_sheet(&mut self) {
        if let Some(mut p) = self.sheet_metal.take() {
            p.restore_filter(self);
        }
    }
    pub(super) fn open_sheet(&mut self, id: String, create: bool) {
        self.close_sheet();
        self.sheet_metal = Some(Panel {
            path: self.root.join(format!("{id}.dxf")).display().to_string(),
            part: id,
            mode: if create {
                Mode::New(Default::default())
            } else {
                Mode::View
            },
            previous_filter: None,
            expected: create.then(|| self.data.clone()),
            saving: false,
            flat: false,
            bends: true,
            revision: None,
            read: None,
            #[cfg(test)]
            read_gate: None,
            result: None,
            current: false,
            edit_current: true,
            export: None,
            message: None,
            zoom: 1.,
            pan: Vec2::ZERO,
        });
    }
    pub(super) fn sheet_buttons(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.pending.is_none() && self.design_source_name.is_none(),
                egui::Button::new("New sheet metal"),
            )
            .on_disabled_hover_text(
                self.design_source_name
                    .as_ref()
                    .map(|s| format!("Add the sheet in {s}, then build the project."))
                    .unwrap_or_default(),
            )
            .clicked()
        {
            let id = (1..)
                .map(|n| format!("sheet_{n}"))
                .find(|id| !self.data.parts.contains_key(id))
                .unwrap();
            self.open_sheet(id, true);
        }
        if let Some(id) = self.selected.clone()
            && recipe(&self.data, &id).is_some()
            && ui.button("Sheet metal").clicked()
        {
            self.open_sheet(id, false);
        }
    }
    pub(super) fn sheet_inspector(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) -> bool {
        let Some(mut p) = self.sheet_metal.take() else {
            return false;
        };
        if p.saving && self.pending.is_none() {
            p.saving = false;
            if self.error.is_none() {
                p.restore_filter(self);
                self.select_only(Some(p.part.clone()));
                p.expected = None;
                p.mode = Mode::View;
                p.revision = None;
            }
        }
        let create = matches!(p.mode, Mode::New(_));
        if !create && recipe(&self.data, &p.part).is_none() {
            p.restore_filter(self);
            return false;
        }
        if !create && self.pending.is_none() {
            p.read(self, ui.ctx());
        }
        ui.heading(if create {
            "New sheet"
        } else {
            "Sheet metal part"
        });
        if let Some(source) = &self.design_source_name {
            ui.label(format!(
                "Edit the sheet in {source}, then build the project."
            ));
        }
        let mut action = Action::None;
        let idle = self.pending.is_none() && !p.saving;
        let g = recipe(&self.data, &p.part);
        let fields = if let Some(Geometry::SheetMetal { rule, flanges, .. }) = g {
            Some((rule, flanges))
        } else {
            None
        };
        let editable = self.design_source_name.is_none()
            && self.data.parts.get(&p.part).is_some_and(|part| {
                faber_core::pick_selector::editable_part(&p.part, part).is_ok()
            });
        ui.add_enabled_ui(idle, |ui| {
            match &mut p.mode {
                Mode::New(f) | Mode::Rule(f) => {
                    f.controls(ui, create);
                    if ui
                        .add_enabled(
                            self.design_source_name.is_none() && p.edit_current,
                            egui::Button::new(if create { "Create sheet" } else { "Save rule" }),
                        )
                        .clicked()
                    {
                        action = Action::Save;
                    }
                }
                Mode::Flange(f) => {
                    f.controls(ui, p.current);
                    if ui
                        .add_enabled(
                            editable && p.current && p.edit_current && f.target.is_some(),
                            egui::Button::new("Save flange"),
                        )
                        .clicked()
                    {
                        action = Action::Save;
                    }
                }
                Mode::View => {
                    if let Some((rule, flanges)) = fields {
                        ui.label(&self.data.parts[&p.part].name);
                        ui.label(format!(
                            "Thickness {} mm · radius {} mm · K {}",
                            form::value(&rule.thickness),
                            form::value(&rule.bend_radius),
                            rule.k_factor
                                .as_ref()
                                .map(form::value)
                                .unwrap_or_else(|| "0.44".into())
                        ));
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(editable, egui::Button::new("Edit sheet rule"))
                                .clicked()
                            {
                                action = Action::Rule;
                            }
                            if ui
                                .add_enabled(editable && p.current, egui::Button::new("Add flange"))
                                .clicked()
                            {
                                action = Action::Add;
                            }
                        });
                        ui.separator();
                        ui.checkbox(&mut p.flat, "Flat pattern");
                        if p.flat {
                            let choice = p.pattern_controls(ui);
                            if !matches!(choice, Action::None) {
                                action = choice;
                            }
                        }
                        for (i, f) in flanges.iter().enumerate() {
                            ui.separator();
                            if ui
                                .add_enabled(editable, egui::Button::new(format!("Edit {}", f.id)))
                                .clicked()
                            {
                                action = Action::Edit(i);
                            }
                            ui.label(format!(
                                "{} · edge {} · {} mm · {}°",
                                f.on,
                                f.edge,
                                form::value(&f.length),
                                f.angle_deg
                                    .as_ref()
                                    .map(form::value)
                                    .unwrap_or_else(|| "90".into())
                            ));
                            ui.label(format!(
                                "Insets {} / {} mm",
                                f.inset_start
                                    .as_ref()
                                    .map(form::value)
                                    .unwrap_or_else(|| "0".into()),
                                f.inset_end
                                    .as_ref()
                                    .map(form::value)
                                    .unwrap_or_else(|| "0".into())
                            ));
                        }
                        if let Some(r) = &p.result {
                            for msg in r.pattern.warnings.iter().chain(&r.pattern.problems) {
                                ui.colored_label(egui::Color32::LIGHT_YELLOW, msg);
                            }
                        }
                    }
                }
            }
            if matches!(p.mode, Mode::View) {
                if ui.button("Close sheet metal").clicked() {
                    action = Action::Close;
                }
            } else if ui.button("Cancel sheet edit").clicked() {
                action = if create {
                    Action::Close
                } else {
                    Action::Cancel
                };
            }
        });
        if let Some(s) = p.message.as_ref().or(self.error.as_ref()) {
            ui.label(s);
        }
        if !p.edit_current && p.expected.is_some() {
            ui.label("The project changed. Cancel the edit and open it again.");
        }
        if !create && !p.current {
            if p.result.is_some() {
                ui.label("Showing the previous sheet revision.");
            }
            if p.read.is_some() {
                ui.label("Updating flat pattern…");
            } else if ui.button("Read sheet again").clicked() {
                p.revision = None;
            }
        }
        match action {
            Action::Close => {
                p.restore_filter(self);
                return true;
            }
            Action::Cancel => {
                p.restore_filter(self);
                p.mode = Mode::View;
                p.expected = None;
                p.message = None;
            }
            Action::Save => {
                if p.expected.as_ref() != Some(&self.data) {
                    p.message =
                        Some("The project changed. Cancel the edit and open it again.".into());
                } else {
                    match p.make_edit(&self.data) {
                        Ok(edit) => {
                            self.save_sections(ui.ctx(), vec![edit]);
                            p.saving = true;
                        }
                        Err(e) => p.message = Some(e),
                    }
                }
            }
            Action::Rule | Action::Add | Action::Edit(_) => {
                if let Some((rule, flanges)) = fields {
                    p.expected = Some(self.data.clone());
                    p.edit_current = true;
                    p.flat = false;
                    p.message = None;
                    p.mode = match action {
                        Action::Rule => Mode::Rule(RuleForm::new(rule)),
                        Action::Edit(i) => Mode::Flange(FlangeForm::edit(i, &flanges[i])),
                        _ => {
                            let draft = FlangeForm::new(flanges);
                            p.previous_filter = Some(self.select_filter);
                            self.set_filter(SelectFilter::Edge);
                            Mode::Flange(draft)
                        }
                    };
                }
            }
            Action::Export => p.export(self, ui.ctx()),
            Action::Browse => {
                self.dialog = Some(dialogs::flat_pattern_file(
                    frame,
                    self.root.clone(),
                    p.part.clone(),
                ))
            }
            Action::None => {}
        }
        self.sheet_metal = Some(p);
        true
    }
    pub(super) fn sheet_pick(&mut self, rect: egui::Rect, pos: egui::Pos2) -> bool {
        let Some(p) = &self.sheet_metal else {
            return false;
        };
        if !matches!(&p.mode,Mode::Flange(f) if f.index.is_none()) {
            return false;
        }
        if !p.current || self.pending.is_some() || !p.edit_current {
            return true;
        }
        let selected = self.pick_entity(rect, pos).and_then(|(pick, point)| {
            let Pick::Edge { part, edge } = pick else {
                return None;
            };
            if part != p.part {
                return None;
            }
            let pts = self
                .bodies
                .iter()
                .find(|b| b.id == part)?
                .edge_lines
                .edge(edge)?;
            let wall = p.result.as_ref()?.edges.iter().find(|e| {
                [&e.ends, &e.other]
                    .into_iter()
                    .any(|segment| on_segment(pts, segment))
            })?;
            Some((
                (wall.wall.clone(), wall.edge),
                Pick::Edge { part, edge },
                point,
            ))
        });
        if let Some(p) = &mut self.sheet_metal
            && let Mode::Flange(f) = &mut p.mode
        {
            p.message = selected
                .is_none()
                .then(|| "Select a free straight edge of this sheet.".into());
            self.picks.clear();
            self.pick_point = None;
            if let Some((wall, pick, point)) = selected {
                f.target = Some(wall);
                self.picks.push(pick);
                self.pick_point = Some(point);
            } else {
                f.target = None;
            }
            self.dirty = true;
        }
        true
    }
}
fn on_segment(points: &[[f64; 3]], ends: &[[f64; 3]; 2]) -> bool {
    let [a, b] = ends.map(glam::DVec3::from);
    let line = b - a;
    let len = line.length_squared();
    let tolerance = 1e-6 * (1. + line.length());
    len > 1e-12
        && points.len() >= 2
        && points.iter().all(|p| {
            let p = glam::DVec3::from(*p);
            let t = ((p - a).dot(line) / len).clamp(0., 1.);
            (p - a - line * t).length_squared() <= tolerance * tolerance
        })
}
