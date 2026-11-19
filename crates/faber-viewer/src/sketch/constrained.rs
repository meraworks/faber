use super::constraints::{Label, Pick, RULES, Rule, number};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use faber_model::{
    Scalar, SketchConstraint as Constraint, SketchEntity as Entity, SolvedSketchData as Sketch,
};
use faber_sketch::{Solution, Status};
use std::collections::BTreeMap;

const FREE: Color32 = Color32::from_rgb(26, 98, 190);
const CONFLICT: Color32 = Color32::from_rgb(196, 45, 48);
const SELECTED: Color32 = Color32::from_rgb(176, 92, 0);

#[derive(Copy, Clone, PartialEq, Eq)]
enum Tool {
    Select,
    Line,
    Circle,
    Arc,
    Rectangle,
}
struct Dimension {
    index: Option<usize>,
    rule: &'static Rule,
    fields: serde_json::Map<String, serde_json::Value>,
    text: String,
}

pub(crate) struct Editor {
    pub target: Option<(String, String, String)>,
    pub data: Sketch<Scalar>,
    params: Result<BTreeMap<String, f64>, String>,
    solution: Result<Solution, String>,
    last_valid: Option<Solution>,
    labels: Vec<Label>,
    tool: Tool,
    picks: Vec<Pick>,
    draft: Vec<([f64; 2], Option<String>)>,
    dimension: Option<Dimension>,
    pub dirty: bool,
    notice: Option<String>,
    scale: f32,
    origin: [f64; 2],
}

impl Editor {
    pub fn new(
        data: Sketch<Scalar>,
        params: &BTreeMap<String, String>,
        target: Option<(String, String, String)>,
    ) -> Self {
        let mut e = Self {
            params: faber_expr::resolve(params),
            target,
            data,
            solution: Err(String::new()),
            last_valid: None,
            labels: Vec::new(),
            tool: Tool::Select,
            picks: vec![],
            draft: vec![],
            dimension: None,
            dirty: false,
            notice: None,
            scale: 4.,
            origin: [0.; 2],
        };
        e.solve();
        if let Ok(s) = &e.solution
            && !s.points.is_empty()
        {
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for p in s.points.values() {
                for i in 0..2 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            for (id, r) in &s.radii {
                if let Some(Entity::Circle { centre, .. }) = e.data.entities.get(id)
                    && let Some(p) = s.points.get(centre)
                {
                    for i in 0..2 {
                        lo[i] = lo[i].min(p[i] - r);
                        hi[i] = hi[i].max(p[i] + r);
                    }
                }
            }
            e.origin = [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2.];
            e.scale = (350. / (hi[0] - lo[0]).max(1.))
                .min(190. / (hi[1] - lo[1]).max(1.))
                .clamp(0.0001, 8.) as f32;
        }
        e
    }
    fn numeric(&self) -> Result<Sketch, String> {
        let params = self.params.as_ref().map_err(Clone::clone)?;
        self.data.map_numbers(&mut |v| number(v, params))
    }
    fn solve(&mut self) {
        self.labels = self.data.constraints.iter().map(Label::new).collect();
        let solved = self.numeric().and_then(|s| faber_sketch::solve(&s));
        if let Ok(prior) = std::mem::replace(&mut self.solution, solved) {
            self.last_valid = self.solution.is_err().then_some(prior);
        }
    }
    fn changed(&mut self) {
        self.dirty = true;
        self.notice = None;
        self.solve();
    }
    pub fn ready(&self) -> bool {
        !self.data.entities.is_empty()
            && self.dimension.is_none()
            && self
                .solution
                .as_ref()
                .is_ok_and(|s| s.status != Status::Over)
    }
    fn take_point(&mut self, at: [f64; 2], existing: Option<String>, preferred: &str) -> String {
        if let Some(id) = existing {
            return id;
        }
        let mut id = preferred.to_owned();
        let mut n = 1;
        while self.data.points.contains_key(&id) {
            id = format!("{preferred}{n}");
            n += 1;
        }
        self.data.points.insert(id.clone(), at.map(Scalar::from));
        id
    }
    fn add_entity(&mut self, prefix: &str, e: Entity<Scalar>) -> String {
        let id = (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| !self.data.entities.contains_key(id))
            .unwrap();
        self.data.entities.insert(id.clone(), e);
        id
    }
    fn add_at(&mut self, at: [f64; 2], existing: Option<String>) {
        self.draft.push((at, existing));
        let required = match self.tool {
            Tool::Select => return,
            Tool::Arc => 3,
            _ => 2,
        };
        if self.draft.len() < required {
            return;
        }
        let mut clicks = std::mem::take(&mut self.draft).into_iter();
        let (p, a) = clicks.next().unwrap();
        let (q, b) = clicks.next().unwrap();
        if (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-7 {
            self.notice = Some("Choose two different points.".into());
            return;
        }
        match self.tool {
            Tool::Line => {
                let a = self.take_point(p, a, "p");
                let b = self.take_point(q, b, "p");
                self.add_entity("line", Entity::Line { a, b });
            }
            Tool::Circle => {
                let centre = self.take_point(p, a, "p");
                self.add_entity(
                    "circle",
                    Entity::Circle {
                        centre,
                        radius: (p[0] - q[0]).hypot(p[1] - q[1]).into(),
                    },
                );
            }
            Tool::Arc => {
                let (r, c) = clicks.next().unwrap();
                let centre = self.take_point(p, a, "p");
                let start = self.take_point(q, b, "p");
                let end = self.take_point(r, c, "p");
                self.add_entity("arc", Entity::Arc { centre, start, end });
            }
            Tool::Rectangle => {
                let lo = [p[0].min(q[0]), p[1].min(q[1])];
                let hi = [p[0].max(q[0]), p[1].max(q[1])];
                if hi[0] - lo[0] < 1e-7 || hi[1] - lo[1] < 1e-7 {
                    self.notice = Some("Choose opposite corners.".into());
                    return;
                }
                let a = self.take_point(lo, None, "a");
                let b = self.take_point([hi[0], lo[1]], None, "b");
                let c = self.take_point(hi, None, "c");
                let d = self.take_point([lo[0], hi[1]], None, "d");
                for (from, to, horizontal) in [
                    (&a, &b, true),
                    (&b, &c, false),
                    (&c, &d, true),
                    (&d, &a, false),
                ] {
                    let line = self.add_entity(
                        "line",
                        Entity::Line {
                            a: from.clone(),
                            b: to.clone(),
                        },
                    );
                    self.data.constraints.push(if horizontal {
                        Constraint::Horizontal { line }
                    } else {
                        Constraint::Vertical { line }
                    });
                }
                self.data.constraints.extend([
                    Constraint::Fix { point: a.clone() },
                    Constraint::DistanceX {
                        a: a.clone(),
                        b: b.clone(),
                        value: (hi[0] - lo[0]).into(),
                    },
                    Constraint::DistanceY {
                        a,
                        b: d,
                        value: (hi[1] - lo[1]).into(),
                    },
                ]);
                self.tool = Tool::Select;
            }
            Tool::Select => {}
        }
        self.changed();
    }

    fn dim(&mut self, index: usize) {
        let label = &self.labels[index];
        if let Some(value) = &label.value {
            let serde_json::Value::Object(fields) =
                serde_json::json!(&self.data.constraints[index])
            else {
                return;
            };
            self.dimension = Some(Dimension {
                index: Some(index),
                rule: label.rule,
                fields,
                text: value.clone(),
            });
        }
    }
    fn add_constraint(&mut self, c: Constraint<Scalar>) {
        if matches!(c, Constraint::Fix { .. }) {
            use faber_sketch::edit::{SketchOp, apply_parametric};
            let updated = self
                .params
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|params| {
                    apply_parametric(
                        &self.data,
                        &[SketchOp::AddConstraint { constraint: c }],
                        &mut |v| number(v, params),
                    )
                });
            match updated {
                Ok(data) => self.data = data,
                Err(message) => {
                    self.notice = Some(message);
                    return;
                }
            }
        } else {
            self.data.constraints.push(c);
        }
        self.picks.clear();
        self.changed();
    }

    fn apply_dimension(&mut self) {
        let Some(d) = &self.dimension else {
            return;
        };
        match d.rule.make(d.fields.clone(), &d.text) {
            Ok(c) => {
                if let Some(index) = d.index {
                    self.data.constraints[index] = c;
                } else {
                    self.data.constraints.push(c);
                }
                self.dimension = None;
                self.picks.clear();
                self.changed();
            }
            Err(error) => self.notice = Some(error),
        }
    }

    fn toggle(&mut self, pick: Pick) {
        if let Some(i) = self.picks.iter().position(|p| *p == pick) {
            self.picks.remove(i);
        } else {
            self.picks.push(pick);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tool, label) in [
                (Tool::Select, "Select"),
                (Tool::Line, "Line"),
                (Tool::Circle, "Circle"),
                (Tool::Arc, "Arc"),
                (Tool::Rectangle, "Rectangle"),
            ] {
                if ui.selectable_value(&mut self.tool, tool, label).changed() {
                    self.draft.clear();
                    self.picks.clear();
                }
            }
            ui.menu_button("Constraints", |ui| {
                ui.horizontal_top(|ui| {
                    for rules in [&RULES[..7], &RULES[7..14], &RULES[14..]] {
                        ui.vertical(|ui| {
                            for rule in rules {
                                let fields = rule.bind(&self.picks, &self.data);
                                let help = rule
                                    .inputs
                                    .iter()
                                    .map(|&(_, kind)| kind.label())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                if ui
                                    .add_enabled(fields.is_some(), egui::Button::new(rule.label))
                                    .on_disabled_hover_text(format!("Select {help}."))
                                    .clicked()
                                    && let Some(fields) = fields
                                {
                                    if rule.dimension.is_some() {
                                        self.dimension = Some(Dimension {
                                            index: None,
                                            rule,
                                            fields,
                                            text: String::new(),
                                        });
                                    } else if let Ok(c) = rule.make(fields, "") {
                                        self.add_constraint(c);
                                    }
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            });
            if ui
                .add_enabled(
                    self.picks.iter().any(|p| matches!(p, Pick::Entity(_))),
                    egui::Button::new("Construction"),
                )
                .clicked()
            {
                for p in &self.picks {
                    if let Pick::Entity(id) = p {
                        if self.data.construction.contains(id) {
                            self.data.construction.retain(|i| i != id);
                        } else {
                            self.data.construction.push(id.clone());
                        }
                    }
                }
                self.changed();
            }
        });
        ui.small(match self.tool{Tool::Select=>"Select points and entities, then choose a constraint. Double-click a dimension to edit it.",Tool::Line=>"Click the two ends of the line.",Tool::Circle=>"Click the centre and a point on the circle.",Tool::Arc=>"Click the centre, start and end in counter-clockwise order.",Tool::Rectangle=>"Click two opposite corners."});
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.toolbar(ui);
        self.canvas(ui);
        if let Some(d) = &mut self.dimension {
            let mut apply = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                let label = ui.label("Dimension value");
                let field = ui
                    .add(egui::TextEdit::singleline(&mut d.text).desired_width(155.))
                    .labelled_by(label.id);
                ui.label(if d.rule.kind == "angle" { "°" } else { "mm" });
                apply = ui.button("Set dimension").clicked()
                    || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                cancel = ui.button("Cancel value").clicked();
            });
            if apply {
                self.apply_dimension();
            } else if cancel {
                self.dimension = None;
            }
        }
        match &self.solution {
            Ok(s) if s.status == Status::Over => {
                ui.colored_label(CONFLICT, "Conflicting constraints");
                if let Some(limit) = &s.diagnosis_limit {
                    ui.small(limit);
                }
            }
            Ok(s) => {
                ui.label(match s.dof {
                    0 => "Fully constrained · 0 degrees of freedom".into(),
                    1 => "1 degree of freedom".into(),
                    n => format!("{n} degrees of freedom"),
                });
            }
            Err(msg) => {
                ui.colored_label(CONFLICT, msg);
            }
        }
        if let Some(msg) = &self.notice {
            ui.colored_label(CONFLICT, msg);
        }
        let mut remove = None;
        let mut edit = None;
        egui::ScrollArea::vertical()
            .id_salt("sketch_constraints")
            .max_height(240.)
            .show(ui, |ui| {
                for (i, l) in self.labels.iter().enumerate() {
                    let bad = self
                        .solution
                        .as_ref()
                        .is_ok_and(|s| s.conflicting.contains(&i));
                    ui.horizontal(|ui| {
                        let text = format!(
                            "{} · {} {}",
                            i + 1,
                            l.rule.label,
                            l.picks
                                .iter()
                                .map(|p| p.id())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        ui.colored_label(
                            if bad {
                                CONFLICT
                            } else {
                                ui.visuals().text_color()
                            },
                            text,
                        );
                        if let Some(value) = &l.value
                            && ui.button(value).on_hover_text("Edit dimension").clicked()
                        {
                            edit = Some(i);
                        }
                        if self
                            .solution
                            .as_ref()
                            .is_ok_and(|s| s.redundant.contains(&i))
                        {
                            ui.small("Redundant");
                        }
                        if ui
                            .small_button(format!("Remove constraint {}", i + 1))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = remove {
            self.dimension = None;
            self.data.constraints.remove(i);
            self.changed();
        }
        if let Some(i) = edit {
            self.dim(i);
        }
    }
}

impl Editor {
    fn drag_point(&mut self, id: &str, mut target: [f64; 2]) -> Result<(), String> {
        let Some(before) = self
            .solution
            .as_ref()
            .ok()
            .and_then(|s| s.points.get(id).copied())
        else {
            return Ok(());
        };
        let driven = self.data.points[id]
            .each_ref()
            .map(|v| matches!(v, Scalar::Expression(_)));
        if driven.into_iter().all(|x| x) {
            return Ok(());
        }
        for i in 0..2 {
            if driven[i] {
                target[i] = before[i];
            }
        }
        let moved = faber_sketch::drag(&self.numeric()?, id, target)?;
        if moved.status == Status::Over {
            return Ok(());
        }
        let mut candidate = self.data.clone();
        for (id, xy) in &mut candidate.points {
            for (seed, coordinate) in xy.iter_mut().zip(moved.points[id]) {
                if let Scalar::Number(v) = seed {
                    *v = coordinate;
                }
            }
        }
        for (id, entity) in &mut candidate.entities {
            if let Entity::Circle {
                radius: Scalar::Number(value),
                ..
            } = entity
            {
                *value = moved.radii[id];
            }
        }
        // Use the saved seeds to resolve the same positions after reopening.
        let params = self.params.as_ref().map_err(Clone::clone)?;
        let numeric = candidate.map_numbers(&mut |v| number(v, params))?;
        let saved = faber_sketch::solve(&numeric)?;
        if saved.status != Status::Over
            && (0..2).all(|i| !driven[i] || (saved.points[id][i] - before[i]).abs() < 1e-8)
        {
            self.data = candidate;
            self.solution = Ok(saved);
            self.dirty = true;
        }
        Ok(())
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, area) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 265.),
            Sense::click_and_drag(),
        );
        area.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Constraint canvas")
        });
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3., Color32::from_rgb(241, 244, 246));
        let centre = rect.center();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let world = |p: Pos2, origin: [f64; 2], scale: f32| {
            [
                origin[0] + f64::from((p.x - centre.x) / scale),
                origin[1] + f64::from((centre.y - p.y) / scale),
            ]
        };
        if let Some(p) = pointer.filter(|p| rect.contains(*p)) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                let fixed = world(p, self.origin, self.scale);
                self.scale = (self.scale * (scroll * 0.002).exp()).clamp(0.0001, 300.);
                let shifted = world(p, self.origin, self.scale);
                for i in 0..2 {
                    self.origin[i] += fixed[i] - shifted[i];
                }
            }
        }
        let origin = self.origin;
        let scale = self.scale;
        let screen = |p: [f64; 2]| {
            centre
                + egui::vec2(
                    (p[0] - origin[0]) as f32 * scale,
                    (origin[1] - p[1]) as f32 * scale,
                )
        };
        let zero = screen([0., 0.]);
        let axis = Stroke::new(1., Color32::from_gray(181));
        if rect.x_range().contains(zero.x) {
            painter.line_segment(
                [
                    egui::pos2(zero.x, rect.top()),
                    egui::pos2(zero.x, rect.bottom()),
                ],
                axis,
            );
        }
        if rect.y_range().contains(zero.y) {
            painter.line_segment(
                [
                    egui::pos2(rect.left(), zero.y),
                    egui::pos2(rect.right(), zero.y),
                ],
                axis,
            );
        }
        let numeric = self
            .solution
            .is_err()
            .then(|| self.numeric().ok())
            .flatten();
        let result = self.solution.as_ref().ok().or(self.last_valid.as_ref());
        let points = result
            .map(|s| &s.points)
            .or_else(|| numeric.as_ref().map(|s| &s.points));
        let Some(points) = points else {
            return;
        };
        let mut positions = BTreeMap::new();
        let mut nearest: Option<(f32, Pick)> = None;
        let mut point_action = None;
        let mut drag_action = None;
        let mut dimension = None;
        let mut pick = None;
        let problem = result
            .map(|s| {
                s.conflicting
                    .iter()
                    .filter_map(|i| self.labels.get(*i))
                    .flat_map(|l| l.picks.iter())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (id, entity) in &self.data.entities {
            let item = Pick::Entity(id.clone());
            let mut vertices = Vec::new();
            let mut ids = Vec::new();
            match entity {
                Entity::Line { a, b } => {
                    ids.extend([a, b]);
                    if let (Some(a), Some(b)) = (points.get(a), points.get(b)) {
                        vertices.extend([screen(*a), screen(*b)]);
                    }
                }
                Entity::Circle { centre: c, .. } | Entity::Arc { centre: c, .. } => {
                    ids.push(c);
                    if let Some(&o) = points.get(c) {
                        let params = match entity {
                            Entity::Circle { radius, .. } => result
                                .and_then(|s| s.radii.get(id).copied())
                                .or_else(|| {
                                    self.params
                                        .as_ref()
                                        .ok()
                                        .and_then(|p| number(radius, p).ok())
                                })
                                .map(|r| (r, 0., std::f64::consts::TAU)),
                            Entity::Arc { start, end, .. } => {
                                ids.extend([start, end]);
                                points.get(start).zip(points.get(end)).map(|(a, b)| {
                                    let start = (a[1] - o[1]).atan2(a[0] - o[0]);
                                    let end = (b[1] - o[1]).atan2(b[0] - o[0]);
                                    (
                                        (a[0] - o[0]).hypot(a[1] - o[1]),
                                        start,
                                        (end - start).rem_euclid(std::f64::consts::TAU),
                                    )
                                })
                            }
                            _ => None,
                        };
                        if let Some((r, start, sweep)) = params {
                            let n = (sweep * (r * f64::from(scale)).abs().sqrt())
                                .ceil()
                                .clamp(12., 600.) as usize;
                            for j in 0..=n {
                                let t = start + sweep * j as f64 / n as f64;
                                vertices.push(screen([o[0] + r * t.cos(), o[1] + r * t.sin()]));
                            }
                        }
                    }
                }
            }
            if vertices.len() < 2 {
                continue;
            }
            let conflict = problem.contains(&&item)
                || ids
                    .iter()
                    .any(|p| problem.contains(&&Pick::Point((*p).clone())));
            let free = result.is_none_or(|s| {
                ids.iter().any(|p| s.free_points.contains(*p)) || s.free_radii.contains(id)
            });
            let colour = if conflict {
                CONFLICT
            } else if self.picks.contains(&item) {
                SELECTED
            } else if free {
                FREE
            } else {
                Color32::BLACK
            };
            let stroke = Stroke::new(2., colour);
            let mid = if vertices.len() == 2 {
                vertices[0].lerp(vertices[1], 0.5)
            } else {
                vertices[vertices.len() / 2]
            };
            positions.insert((false, id.clone()), mid);
            if let Some(p) = pointer {
                let distance = vertices
                    .windows(2)
                    .map(|s| {
                        let d = s[1] - s[0];
                        p.distance(
                            s[0] + d * ((p - s[0]).dot(d) / d.length_sq().max(1e-12)).clamp(0., 1.),
                        )
                    })
                    .fold(f32::INFINITY, f32::min);
                if distance < 8. && nearest.as_ref().is_none_or(|(old, _)| distance < *old) {
                    nearest = Some((distance, item));
                }
            }
            if self.data.construction.contains(id) {
                painter.extend(egui::Shape::dashed_line(&vertices, stroke, 6., 4.));
            } else {
                painter.add(egui::Shape::line(vertices, stroke));
            }
        }
        for (id, &point) in points {
            let pos = screen(point);
            positions.insert((true, id.clone()), pos);
            let item = Pick::Point(id.clone());
            let conflict = problem.contains(&&item);
            let colour = if conflict {
                CONFLICT
            } else if self.picks.contains(&item) {
                SELECTED
            } else if result.is_none_or(|s| s.free_points.contains(id)) {
                FREE
            } else {
                Color32::BLACK
            };
            painter.circle_filled(pos, 4., colour);
            if !rect.contains(pos) {
                continue;
            }
            if self.tool == Tool::Select {
                let resp = ui.interact(
                    Rect::from_center_size(pos, Vec2::splat(14.)),
                    ui.scope_id().with(("sketch_point", id)),
                    Sense::click_and_drag(),
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Point {id}"))
                });
                if resp.clicked() {
                    pick = Some(item);
                }
                if (resp.dragged() || resp.drag_stopped())
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    drag_action = Some((id.clone(), world(p, origin, scale)));
                }
            }
            if pointer.is_some_and(|p| p.distance(pos) < 8.) {
                point_action = Some((point, id.clone()));
            }
        }
        let mut used: Vec<Rect> = vec![];
        for (i, l) in self.labels.iter().enumerate() {
            let p: Vec<_> = l
                .picks
                .iter()
                .filter_map(|p| positions.get(&(matches!(p, Pick::Point(_)), p.id().to_owned())))
                .collect();
            if p.is_empty() {
                continue;
            }
            let at = p.iter().fold(Vec2::ZERO, |s, p| s + p.to_vec2()) / p.len() as f32;
            let mut at = Pos2::ZERO + at + egui::vec2(8., -14.);
            let caption = l.value.as_ref().map_or_else(
                || glyph(l.rule.kind).to_string(),
                |v| format!("{v} {}", if l.rule.kind == "angle" { "°" } else { "mm" }),
            );
            let colour = if result.is_some_and(|s| s.conflicting.contains(&i)) {
                CONFLICT
            } else {
                Color32::from_rgb(47, 66, 74)
            };
            let text = painter.layout_no_wrap(caption, egui::FontId::proportional(11.), colour);
            let size = text.size() + Vec2::new(5., 2.);
            let mut bounds = Rect::from_min_size(at, size);
            while used.iter().any(|r| r.intersects(bounds)) {
                at.y -= size.y + 2.;
                bounds = Rect::from_min_size(at, size);
            }
            used.push(bounds);
            painter.rect_filled(bounds, 2., Color32::from_gray(251));
            painter.galley(at + Vec2::new(2., 0.), text, colour);
            if l.value.is_some() && rect.contains(bounds.center()) {
                let hit = ui.interact(bounds, ui.scope_id().with(("dimension", i)), Sense::click());
                hit.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        format!("Dimension {}", i + 1),
                    )
                });
                if hit.double_clicked() || hit.triple_clicked() {
                    dimension = Some(i);
                }
            }
        }
        if let Some((id, target)) = drag_action {
            if let Err(message) = self.drag_point(&id, target) {
                self.notice = Some(message);
            }
        } else if let Some(i) = dimension {
            self.dim(i);
        } else if let Some(p) = pick {
            self.toggle(p);
        } else if area.clicked()
            && let Some(p) = area.interact_pointer_pos()
        {
            if self.tool == Tool::Select {
                if let Some((_, item)) = nearest {
                    self.toggle(item);
                } else {
                    self.picks.clear();
                }
            } else {
                let (v, id) =
                    point_action.map_or((world(p, origin, scale), None), |(v, id)| (v, Some(id)));
                self.add_at(v, id);
            }
        }
        let mut last = None;
        for (p, _) in &self.draft {
            let at = screen(*p);
            painter.circle_filled(at, 3., SELECTED);
            if let Some(prev) = last {
                painter.line_segment([prev, at], Stroke::new(1., SELECTED));
            }
            last = Some(at);
        }
    }
}

fn glyph(kind: &str) -> &'static str {
    match kind {
        "horizontal" => "H",
        "vertical" => "V",
        "fix" => "Fix",
        "parallel" => "∥",
        "perpendicular" => "⊥",
        "coincident" => "Coin",
        "equal" => "=",
        "midpoint" => "Mid",
        "tangent" => "Tan",
        "symmetric" => "Sym",
        "point_on_line" => "On line",
        "point_on_circle" => "On circle",
        "concentric" => "Centre",
        "collinear" => "Col",
        _ => "Dim",
    }
}
