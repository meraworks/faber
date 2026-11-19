//! Edit the control cage. Model changes use the same revision guard as other tools.
use super::*;
use faber_form::{FormOp, Select};
use faber_model::{CageData, Geometry, SymmetryPlane};
use faber_wiring::form as saved;
use glam::{DAffine3, DVec3};
use std::sync::{Arc, Weak};
mod canvas;
mod preview;

#[derive(Default, PartialEq, Clone, Copy)]
enum Mode {
    #[default]
    Vertex,
    Edge,
    Face,
}
#[derive(Default, PartialEq, Clone, Copy)]
enum Tool {
    #[default]
    Move,
    Rotate,
    Scale,
}
#[derive(Default, PartialEq, Clone, Copy)]
enum Primitive {
    #[default]
    Box,
    Cylinder,
    Sphere,
    Torus,
}
#[derive(Clone)]
struct Start {
    primitive: Primitive,
    size: [f64; 3],
    radius: f64,
    minor: f64,
    segments: u32,
    solid: bool,
    levels: u32,
}
impl Default for Start {
    fn default() -> Self {
        Self {
            primitive: Default::default(),
            size: [20.; 3],
            radius: 10.,
            minor: 3.,
            segments: 1,
            solid: false,
            levels: 3,
        }
    }
}
impl Start {
    fn cage(&self) -> Result<CageData<f64>, String> {
        match self.primitive {
            Primitive::Box => faber_form::try_form_box(self.size, [self.segments; 3]),
            Primitive::Cylinder => {
                faber_form::try_form_cylinder(self.radius, self.size[2], 12, self.segments)
            }
            Primitive::Sphere => faber_form::try_form_sphere(self.radius, self.segments),
            Primitive::Torus => {
                faber_form::try_form_torus(self.radius, self.minor, 12, (self.segments * 4).max(8))
            }
        }
    }
}
enum Change {
    New(Start),
    Edit(FormOp),
    Settings(Vec<SymmetryPlane>, u32),
}
pub(super) struct Panel {
    part: String,
    feature: Option<String>,
    start: Option<Start>,
    saving: bool,
    source: Weak<Vec<Body>>,
    cage: Option<Arc<CageData<f64>>>,
    frame: DAffine3,
    sym: Vec<SymmetryPlane>,
    levels: u32,
    preview: preview::Worker,
    serial: u64,
    first: u64,
    shown: Option<preview::Wire>,
    ready: bool,
    current_source: bool,
    mode: Mode,
    tool: Tool,
    items: BTreeSet<u32>,
    drag: Option<canvas::Drag>,
    distance: f64,
    sharpness: f64,
    at: f64,
    flatten_axis: usize,
    message: Option<String>,
}
impl Panel {
    fn request(&mut self, op: Option<FormOp>) {
        let Some(cage) = self.cage.as_ref() else {
            return;
        };
        self.serial += 1;
        if op.is_none() {
            self.first = self.serial;
        }
        self.ready = false;
        self.preview.request(preview::Request {
            serial: self.serial,
            cage: cage.clone(),
            op,
            sym: self.sym.clone(),
            frame: self.frame,
        });
    }
    fn selection(&self) -> Select {
        match self.mode {
            Mode::Vertex => Select::Vertices(self.items.iter().copied().collect()),
            Mode::Face => Select::Faces(self.items.iter().copied().collect()),
            Mode::Edge => Select::Edges(
                self.items
                    .iter()
                    .filter_map(|&i| self.shown.as_ref()?.edges.get(i as usize).copied())
                    .collect(),
            ),
        }
    }
    fn mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.items.clear();
            if self.drag.take().is_some() {
                self.request(None);
            }
        }
    }
    fn vertices(&self, cage: &CageData<f64>) -> BTreeSet<u32> {
        match self.selection() {
            Select::Vertices(v) => v.into_iter().collect(),
            Select::Edges(v) => v.into_iter().flatten().collect(),
            Select::Faces(v) => v
                .iter()
                .filter_map(|&i| cage.faces.get(i as usize))
                .flatten()
                .copied()
                .collect(),
        }
    }
    fn refresh(&mut self, app: &mut Shell) {
        let changed = !self.source.ptr_eq(&Arc::downgrade(&app.bodies));
        if changed {
            self.source = Arc::downgrade(&app.bodies);
            self.current_source = false;
            self.drag = None;
            let result = (|| {
                let src = saved::geometry(&app.data, &self.part, self.feature.as_deref())?;
                let Geometry::Form {
                    cage,
                    levels,
                    symmetry,
                    ..
                } = faber_core::resolve_recipe(&app.data, src)?
                else {
                    return Err("Select the form feature again.".into());
                };
                let frame = faber_core::world_placement(&app.data, &self.part)?;
                if self.cage.as_ref().is_none_or(|c| c.faces != cage.faces) {
                    self.items.clear();
                }
                self.cage = Some(Arc::new(cage));
                self.levels = levels.unwrap_or(3);
                self.sym = symmetry;
                self.frame = frame;
                self.request(None);
                Ok::<_, String>(())
            })();
            if let Err(e) = result {
                self.serial += 1;
                self.ready = false;
                self.cage = None;
                self.shown = None;
                self.message = Some(e);
                app.gpu.intermediate = None;
                app.dirty = true;
            }
        }
        while let Ok((id, result)) = self.preview.answers.try_recv() {
            if id < self.first || (id != self.serial && self.drag.is_none()) {
                continue;
            }
            match result {
                Ok((wire, paint)) => {
                    self.shown = Some(wire);
                    app.gpu.intermediate = Some(paint);
                    self.ready = id == self.serial;
                    self.current_source = true;
                }
                Err(e) => {
                    if id == self.serial {
                        self.message = Some(e);
                        self.ready = false;
                    }
                }
            }
            app.dirty = true;
        }
    }
    fn commit(&mut self, app: &mut Shell, ctx: &egui::Context, change: Change) {
        if app.pending.is_some() || self.saving {
            return;
        }
        let expected = app.data.clone();
        let strict = app.strict_edits;
        let root = app.root.clone();
        let part = self.part.clone();
        let feature = self.feature.clone();
        let (tx, rx) = mpsc::channel();
        app.pending = Some(rx);
        app.saving = true;
        app.error = None;
        self.saving = true;
        self.message = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut p = faber_wiring::open(&root, false, None)?;
                if p.data != expected {
                    return Err(
                        "The form changed. Read the current revision before editing.".into(),
                    );
                }
                let rev = saved::Write {
                    revision: expected.revision,
                    strict,
                };
                match change {
                    Change::New(s) => saved::new_form(
                        &mut p,
                        &part,
                        s.cage()?,
                        Some(s.levels),
                        s.solid,
                        vec![],
                        rev,
                    )?,
                    Change::Edit(op) => {
                        saved::edit_form(&mut p, &part, feature.as_deref(), &[op], rev)?
                    }
                    Change::Settings(sym, levels) => {
                        saved::settings(&mut p, &part, feature.as_deref(), sym, Some(levels), rev)?
                    }
                }
                Ok((p.data, p.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl Shell {
    pub(super) fn form_command(&mut self, ctx: &egui::Context, command: keys::Command) -> bool {
        use keys::Command::*;
        let Some(mut p) = self.form.take() else {
            return false;
        };
        let mut close = false;
        let handled = match command {
            Undo | Redo | Fit | Iso | Top | Front | Project(_) => false,
            Escape => {
                if p.drag.take().is_some() {
                    p.request(None);
                } else {
                    close = !p.saving;
                }
                true
            }
            Move => {
                p.tool = Tool::Move;
                true
            }
            Delete => {
                if self.editable_form(&p) && p.mode == Mode::Face && !p.items.is_empty() {
                    let faces = p.items.iter().copied().collect();
                    p.commit(self, ctx, Change::Edit(FormOp::Delete { faces }));
                }
                true
            }
            _ => true,
        };
        if close {
            self.gpu.intermediate = None;
            self.dirty = true;
        } else {
            self.form = Some(p);
        }
        handled
    }
    pub(super) fn close_form(&mut self) {
        if self.form.take().is_some() {
            self.gpu.intermediate = None;
            self.dirty = true;
        }
    }
    pub(super) fn open_form(
        &mut self,
        ctx: &egui::Context,
        part: String,
        feature: Option<String>,
        create: bool,
    ) {
        self.close_surface_feature();
        self.close_sheet();
        self.close_form();
        self.clear_entity_picks();
        self.views.open = false;
        let preview = preview::Worker::new(part.clone(), self.gpu.device.clone(), ctx.clone());
        self.form = Some(Panel {
            part,
            feature,
            start: create.then(Start::default),
            saving: false,
            source: Weak::new(),
            cage: None,
            frame: DAffine3::IDENTITY,
            sym: vec![],
            levels: 3,
            preview,
            serial: 0,
            first: 0,
            shown: None,
            ready: false,
            current_source: false,
            mode: Mode::Vertex,
            tool: Tool::Move,
            items: BTreeSet::new(),
            drag: None,
            distance: 2.,
            sharpness: 2.,
            at: 0.5,
            flatten_axis: 2,
            message: None,
        });
    }
    pub(super) fn new_form_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.design_source_name.is_none() && self.pending.is_none() && self.form.is_none(),
                egui::Button::new("New form"),
            )
            .on_disabled_hover_text("Add the form to the project source.")
            .clicked()
        {
            let id = (1..)
                .map(|n| format!("form_{n}"))
                .find(|id| !self.data.parts.contains_key(id))
                .unwrap();
            self.open_form(ui.ctx(), id, None, true);
        }
    }
    pub(super) fn edit_form_button(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.selected.clone()
            && saved::geometry(&self.data, &id, None).is_ok()
            && ui.button("Edit form").clicked()
        {
            self.open_form(ui.ctx(), id, None, false);
        }
    }
    pub(super) fn form_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mut p) = self.form.take() else {
            return false;
        };
        if p.saving && self.pending.is_none() {
            p.saving = false;
            if let Some(e) = &self.error {
                p.message = Some(e.clone());
                p.request(None);
            } else {
                if p.start.take().is_some() {
                    self.select_only(Some(p.part.clone()));
                }
                p.source = Weak::new();
            }
        }
        if p.start.is_none() && !p.saving && self.selected.as_deref() != Some(&p.part) {
            self.gpu.intermediate = None;
            self.dirty = true;
            return false;
        }
        let mut change = None;
        let finish;
        if let Some(s) = &mut p.start {
            ui.heading("New form");
            ui.horizontal_wrapped(|ui| {
                for (k, label) in [
                    (Primitive::Box, "Box"),
                    (Primitive::Cylinder, "Cylinder"),
                    (Primitive::Sphere, "Sphere"),
                    (Primitive::Torus, "Torus"),
                ] {
                    ui.selectable_value(&mut s.primitive, k, label);
                }
            });
            match s.primitive {
                Primitive::Box => {
                    for (v, l) in s.size.iter_mut().zip(["Width", "Depth", "Height"]) {
                        length(ui, v, l);
                    }
                }
                Primitive::Cylinder => {
                    length(ui, &mut s.radius, "Radius");
                    length(ui, &mut s.size[2], "Height");
                }
                Primitive::Sphere => length(ui, &mut s.radius, "Radius"),
                Primitive::Torus => {
                    length(ui, &mut s.radius, "Major radius");
                    length(ui, &mut s.minor, "Minor radius");
                }
            }
            ui.add(egui::Slider::new(&mut s.segments, 1..=8).text("Segments"));
            ui.add(egui::Slider::new(&mut s.levels, 0..=4).text("Final level"));
            ui.checkbox(&mut s.solid, "Use as solid");
            if ui
                .add_enabled(!p.saving, egui::Button::new("Create form"))
                .clicked()
            {
                change = Some(Change::New(s.clone()));
            }
            finish = ui
                .add_enabled(!p.saving, egui::Button::new("Cancel"))
                .clicked();
        } else {
            p.refresh(self);
            ui.heading("Edit form");
            ui.label(if p.ready || p.drag.is_some() && p.shown.is_some() {
                "Level 2 preview"
            } else {
                "Updating preview…"
            });
            ui.horizontal_wrapped(|ui| {
                for (mode, label) in [
                    (Mode::Vertex, "Vertex · 1"),
                    (Mode::Edge, "Edge · 2"),
                    (Mode::Face, "Face · 3"),
                ] {
                    if ui.selectable_label(mode == p.mode, label).clicked() {
                        p.mode(mode);
                    }
                }
            });
            let n = p.items.len();
            let item = match (p.mode, n) {
                (Mode::Vertex, 1) => "vertex",
                (Mode::Vertex, _) => "vertices",
                (Mode::Edge, 1) => "edge",
                (Mode::Edge, _) => "edges",
                (Mode::Face, 1) => "face",
                _ => "faces",
            };
            ui.label(format!("{n} {item} selected"));
            let can = self.editable_form(&p);
            if let Some(src) = &self.design_source_name {
                ui.label(format!("Edit the form in {src}."));
            }
            ui.add_enabled_ui(can && p.ready, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tool, label) in [
                        (Tool::Move, "Move"),
                        (Tool::Rotate, "Rotate"),
                        (Tool::Scale, "Scale selection"),
                    ] {
                        ui.selectable_value(&mut p.tool, tool, label);
                    }
                });
                if let Some(op) = p.operation_buttons(ui) {
                    change = Some(Change::Edit(op));
                }
                ui.separator();
                for (axis, label) in [
                    (SymmetryPlane::X, "Symmetry X"),
                    (SymmetryPlane::Y, "Symmetry Y"),
                    (SymmetryPlane::Z, "Symmetry Z"),
                ] {
                    let mut on = p.sym.contains(&axis);
                    if ui.checkbox(&mut on, label).changed() {
                        let mut v = p.sym.clone();
                        v.retain(|a| *a != axis);
                        if on {
                            v.push(axis)
                        }
                        change = Some(Change::Settings(v, p.levels));
                    }
                }
                let mut levels = p.levels;
                egui::ComboBox::from_label("Final level")
                    .selected_text(levels.to_string())
                    .show_ui(ui, |ui| {
                        for i in 0..=4 {
                            ui.selectable_value(&mut levels, i, i.to_string());
                        }
                    });
                if levels != p.levels {
                    change = Some(Change::Settings(p.sym.clone(), levels));
                }
            });
            finish = ui
                .add_enabled(!p.saving, egui::Button::new("Finish form"))
                .clicked();
        }
        if let Some(e) = &p.message {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if let Some(c) = change {
            p.commit(self, ui.ctx(), c);
        }
        if finish {
            self.gpu.intermediate = None;
            self.dirty = true;
        } else {
            self.form = Some(p);
        }
        true
    }
    fn editable_form(&self, p: &Panel) -> bool {
        self.pending.is_none()
            && !p.saving
            && self.design_source_name.is_none()
            && self.data.parts.get(&p.part).is_some_and(|b| {
                faber_core::pick_selector::editable_part(&p.part, b).is_ok()
                    && !matches!(b.geometry, Geometry::Instance { .. })
            })
    }
}
fn length(ui: &mut egui::Ui, value: &mut f64, label: &str) {
    ui.horizontal(|ui| {
        let l = ui.label(label);
        ui.add(egui::DragValue::new(value).range(0.001..=1e5).suffix(" mm"))
            .labelled_by(l.id);
    });
}
impl Panel {
    fn operation_buttons(&mut self, ui: &mut egui::Ui) -> Option<FormOp> {
        let select = self.selection();
        let faces = match &select {
            Select::Faces(v) => v.as_slice(),
            _ => &[],
        };
        let edges = match &select {
            Select::Edges(v) => v.as_slice(),
            _ => &[],
        };
        let vertices = match &select {
            Select::Vertices(v) => v.as_slice(),
            _ => &[],
        };
        let mut op = None;
        ui.horizontal(|ui| {
            let l = ui.label("Distance");
            ui.add(
                egui::DragValue::new(&mut self.distance)
                    .speed(0.1)
                    .suffix(" mm"),
            )
            .labelled_by(l.id);
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!faces.is_empty(), egui::Button::new("Extrude"))
                .clicked()
            {
                op = Some(FormOp::Extrude {
                    faces: faces.to_vec(),
                    distance: self.distance,
                });
            }
            if ui
                .add_enabled(!faces.is_empty(), egui::Button::new("Inset"))
                .clicked()
            {
                op = Some(FormOp::Inset {
                    faces: faces.to_vec(),
                    distance: self.distance,
                });
            }
            if ui
                .add_enabled(faces.len() == 2, egui::Button::new("Bridge"))
                .clicked()
            {
                op = Some(FormOp::Bridge {
                    a: faces[0],
                    b: faces[1],
                });
            }
            if ui
                .add_enabled(vertices.len() == 2, egui::Button::new("Weld"))
                .clicked()
            {
                op = Some(FormOp::Weld {
                    a: vertices[0],
                    b: vertices[1],
                });
            }
            if ui
                .add_enabled(!faces.is_empty(), egui::Button::new("Delete faces"))
                .clicked()
            {
                op = Some(FormOp::Delete {
                    faces: faces.to_vec(),
                });
            }
        });
        ui.horizontal(|ui| {
            let l = ui.label("Fraction");
            ui.add(
                egui::DragValue::new(&mut self.at)
                    .range(0.01..=0.99)
                    .speed(0.01),
            )
            .labelled_by(l.id)
            .on_hover_text("Fraction along the edge");
            if ui
                .add_enabled(edges.len() == 1, egui::Button::new("Insert loop"))
                .clicked()
            {
                op = Some(FormOp::InsertLoop {
                    edge: edges[0],
                    at: self.at,
                });
            }
        });
        ui.horizontal(|ui| {
            let l = ui.label("Sharpness");
            ui.add(
                egui::DragValue::new(&mut self.sharpness)
                    .range(0.0..=10.)
                    .speed(0.1),
            )
            .labelled_by(l.id)
            .on_hover_text("Crease sharpness");
            if ui
                .add_enabled(!edges.is_empty(), egui::Button::new("Crease"))
                .clicked()
            {
                op = Some(FormOp::Crease {
                    edges: edges.to_vec(),
                    sharpness: self.sharpness,
                });
            }
        });
        ui.horizontal(|ui| {
            for (i, l) in ["X", "Y", "Z"].iter().enumerate() {
                ui.selectable_value(&mut self.flatten_axis, i, *l);
            }
            if ui
                .add_enabled(!self.items.is_empty(), egui::Button::new("Flatten"))
                .on_hover_text("Flatten onto the selected axis plane through the selection centre")
                .clicked()
                && let Some(c) = &self.cage
            {
                let v = self.vertices(c);
                let mut plane = [0.; 4];
                plane[self.flatten_axis] = 1.;
                plane[3] = v
                    .iter()
                    .map(|&i| c.vertices[i as usize][self.flatten_axis])
                    .sum::<f64>()
                    / v.len() as f64;
                op = Some(FormOp::Flatten { select, plane });
            }
        });
        op
    }
}
