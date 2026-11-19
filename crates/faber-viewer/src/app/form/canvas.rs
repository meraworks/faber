use super::*;
use glam::DMat4;
const AXES: [DVec3; 3] = [DVec3::X, DVec3::Y, DVec3::Z];
const INK: [Color32; 3] = [
    Color32::from_rgb(218, 86, 69),
    Color32::from_rgb(35, 142, 85),
    Color32::from_rgb(54, 110, 204),
];
pub(super) struct Drag {
    start: egui::Pos2,
    centre: DVec3,
    axis: Option<usize>,
    tangent: Option<Vec2>,
    op: Option<FormOp>,
}
struct Projection {
    matrix: DMat4,
    rect: egui::Rect,
}
impl Projection {
    fn at(&self, p: DVec3) -> Option<(egui::Pos2, f64)> {
        let q = self.matrix * p.extend(1.);
        if q.w <= 0. {
            return None;
        }
        let q = q.truncate() / q.w;
        (q.is_finite() && (0.0..=1.).contains(&q.z)).then(|| {
            (
                self.rect.center()
                    + egui::vec2(
                        q.x as f32 * self.rect.width(),
                        -q.y as f32 * self.rect.height(),
                    ) * 0.5,
                q.z,
            )
        })
    }
    fn unproject(&self, pos: egui::Pos2, z: f64) -> DVec3 {
        let d = (pos - self.rect.center()) / (self.rect.size() * 0.5);
        self.matrix
            .inverse()
            .project_point3(DVec3::new(d.x as f64, -d.y as f64, z))
    }
    fn rings(&self, centre: DVec3) -> Option<move_tool::Triad> {
        let (at, z) = self.at(centre)?;
        let size = centre.distance(self.unproject(at + Vec2::X * 70., z));
        move_tool::triad_at(self.rect, self.matrix.as_mat4(), centre, size)
    }
    fn ring_hit(&self, centre: DVec3, at: egui::Pos2) -> Option<(usize, Vec2)> {
        let (mid, _) = self.at(centre)?;
        if mid.distance(at) < 13. {
            return None;
        }
        let triad = self.rings(centre)?;
        triad
            .rings
            .iter()
            .enumerate()
            .flat_map(|(i, ring)| {
                ring.windows(2).filter_map(move |p| {
                    let (d, _) = near_segment(at, p[0], p[1]);
                    let line = p[1] - p[0];
                    (d <= 9. && line.length_sq() > 0.01).then(|| (d, i, line.normalized()))
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i, dir)| (i, dir))
    }
    fn handles(&self, centre: DVec3) -> Vec<(usize, egui::Pos2, egui::Pos2, Vec2)> {
        let Some((origin, _)) = self.at(centre) else {
            return vec![];
        };
        AXES.iter()
            .enumerate()
            .filter_map(|(i, &axis)| {
                let (end, _) = self.at(centre + axis)?;
                let d = end - origin;
                (d.length() > 0.15).then(|| (i, origin, origin + d.normalized() * 56., d))
            })
            .collect()
    }
}
fn near_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> (f32, f32) {
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_sq().max(1e-8)).clamp(0., 1.);
    (p.distance(a + d * t), t)
}
fn depth(p: egui::Pos2, tri: [(egui::Pos2, f64); 3]) -> Option<f64> {
    let [(a, az), (b, bz), (c, cz)] = tri;
    let det = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let d = det(b - a, c - a);
    if d.abs() < 1e-5 {
        return None;
    }
    let u = det(p - a, c - a) / d;
    let v = det(b - a, p - a) / d;
    (u >= 0. && v >= 0. && u + v <= 1.)
        .then(|| az + f64::from(u) * (bz - az) + f64::from(v) * (cz - az))
}
impl Panel {
    fn centre(&self) -> Option<DVec3> {
        let cage = &self.cage.as_ref()?;
        let v = self.vertices(cage);
        if v.is_empty() {
            return None;
        }
        Some(
            v.iter()
                .map(|&i| DVec3::from(cage.vertices[i as usize]))
                .sum::<DVec3>()
                / v.len() as f64,
        )
    }
    fn hit(&self, proj: &Projection, pos: egui::Pos2) -> Option<u32> {
        let w = self.shown.as_ref()?;
        let v = &w.cage.vertices;
        let point = |i: u32| proj.at(v[i as usize].into());
        let candidates: Vec<(usize, f32, f64)> = match self.mode {
            Mode::Face => w
                .cage
                .faces
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let d = (1..f.len() - 1)
                        .filter_map(|j| depth(pos, [point(f[0])?, point(f[j])?, point(f[j + 1])?]))
                        .min_by(f64::total_cmp)?;
                    Some((i, 0., d))
                })
                .collect(),
            Mode::Edge => w
                .edges
                .iter()
                .enumerate()
                .filter_map(|(i, &[a, b])| {
                    let (a, az) = point(a)?;
                    let (b, bz) = point(b)?;
                    let (dist, t) = near_segment(pos, a, b);
                    (dist <= 9.).then_some((i, dist, az + f64::from(t) * (bz - az)))
                })
                .collect(),
            Mode::Vertex => v
                .iter()
                .enumerate()
                .filter_map(|(i, &p)| {
                    let (p, z) = proj.at(p.into())?;
                    let d = p.distance(pos);
                    (d <= 10.).then_some((i, d, z))
                })
                .collect(),
        };
        candidates
            .into_iter()
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|s| s.0 as u32)
    }
    fn delta(&self, proj: &Projection, drag: &Drag, pos: egui::Pos2) -> Option<FormOp> {
        let (origin, z) = proj.at(drag.centre)?;
        let length = pos - drag.start;
        if length.length() < 1. {
            return None;
        }
        let actual = proj.unproject(pos, z) - proj.unproject(drag.start, z);
        let select = self.selection();
        let about = Some(drag.centre.into());
        Some(match self.tool {
            Tool::Move => {
                let by = if let Some(i) = drag.axis {
                    let (end, _) = proj.at(drag.centre + AXES[i])?;
                    let d = end - origin;
                    AXES[i] * f64::from(length.dot(d) / d.length_sq().max(0.001))
                } else {
                    actual
                };
                FormOp::Move {
                    select,
                    by: by.into(),
                }
            }
            Tool::Rotate => {
                let axis = drag.axis.map_or_else(
                    || {
                        let a = proj.unproject(origin + Vec2::X, z) - drag.centre;
                        let b = proj.unproject(origin - Vec2::Y, z) - drag.centre;
                        a.cross(b).normalize_or_zero()
                    },
                    |i| AXES[i],
                );
                let travel = drag.tangent.map_or(length.x - length.y, |t| length.dot(t));
                let degrees = f64::from(travel) * 0.7;
                FormOp::Rotate {
                    select,
                    axis: axis.into(),
                    degrees,
                    about,
                }
            }
            Tool::Scale => {
                let d = if let Some(i) = drag.axis {
                    let (end, _) = proj.at(drag.centre + AXES[i])?;
                    length.dot((end - origin).normalized())
                } else {
                    length.x - length.y
                };
                let s = f64::from((d / 75.).exp().clamp(0.02, 50.));
                let mut factor = [s; 3];
                if let Some(i) = drag.axis {
                    factor = [1.; 3];
                    factor[i] = s;
                }
                FormOp::Scale {
                    select,
                    factor,
                    about,
                }
            }
        })
    }
}
impl Shell {
    fn form_projection(&self, p: &Panel, rect: egui::Rect) -> Projection {
        Projection {
            matrix: self.camera.matrix(rect.aspect_ratio()).as_dmat4()
                * self.display_pose(&p.part).as_dmat4()
                * DMat4::from(p.frame),
            rect,
        }
    }
    pub(in crate::app) fn form_input(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
    ) -> bool {
        let Some(mut p) = self.form.take() else {
            return false;
        };
        if !p.current_source
            || p.cage.is_none()
            || p.shown.is_none()
            || self.hidden.contains(&p.part)
        {
            self.form = Some(p);
            return false;
        }
        let proj = self.form_projection(&p, rect);
        let primary = egui::PointerButton::Primary;
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
        if !ui.ctx().text_edit_focused() {
            for (key, m) in [
                (egui::Key::Num1, Mode::Vertex),
                (egui::Key::Num2, Mode::Edge),
                (egui::Key::Num3, Mode::Face),
            ] {
                if ui.input(|i| i.key_pressed(key)) {
                    p.mode(m);
                }
            }
        }
        let alt = ui.input(|i| i.modifiers.alt);
        if !alt
            && response.clicked_by(primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let hit = p.hit(&proj, pos);
            if !ctrl {
                p.items.clear();
            }
            if let Some(i) = hit
                && (!ctrl || !p.items.remove(&i))
            {
                p.items.insert(i);
            }
        }
        if !alt
            && self.editable_form(&p)
            && response.drag_started_by(primary)
            && let Some(start) = ui.input(|i| i.pointer.press_origin())
        {
            let ring = if p.tool == Tool::Rotate {
                p.centre().and_then(|c| proj.ring_hit(c, start))
            } else {
                None
            };
            let axis = if p.tool == Tool::Rotate {
                ring.map(|r| r.0)
            } else {
                p.centre().and_then(|c| {
                    proj.handles(c)
                        .into_iter()
                        .filter_map(|(i, a, b, _)| {
                            let (d, t) = near_segment(start, a, b);
                            (t > 0.18 && d <= 10.).then_some((i, d))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|v| v.0)
                })
            };
            if axis.is_none()
                && let Some(i) = p.hit(&proj, start)
                && !p.items.contains(&i)
            {
                if !ctrl {
                    p.items.clear();
                }
                p.items.insert(i);
            }
            if (axis.is_some() || p.hit(&proj, start).is_some())
                && let Some(centre) = p.centre()
            {
                p.drag = Some(Drag {
                    start,
                    centre,
                    axis,
                    tangent: ring.map(|r| r.1),
                    op: None,
                });
            }
        }
        if let Some(mut drag) = p.drag.take() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                && (ui.input(|i| i.pointer.delta()) != Vec2::ZERO || drag.op.is_none())
                && let Some(op) = p.delta(&proj, &drag, pos)
            {
                p.request(Some(op.clone()));
                drag.op = Some(op);
            }
            if response.drag_stopped_by(primary) || !ui.input(|i| i.pointer.primary_down()) {
                if let Some(op) = drag.op {
                    p.commit(self, ui.ctx(), Change::Edit(op));
                }
            } else {
                p.drag = Some(drag);
            }
        }
        let consume = p.drag.is_some()
            || (!alt && (response.dragged_by(primary) || response.clicked_by(primary)));
        self.form = Some(p);
        consume
    }
    pub(in crate::app) fn form_paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let Some(p) = self.form.as_ref() else { return };
        let Some(w) = &p.shown else { return };
        if self.hidden.contains(&p.part) {
            return;
        }
        let proj = self.form_projection(p, rect);
        let painter = ui.painter().with_clip_rect(rect);
        let v: Vec<_> = w
            .cage
            .vertices
            .iter()
            .map(|&p| proj.at(p.into()).map(|x| x.0))
            .collect();
        if p.mode == Mode::Face {
            for &i in &p.items {
                if let Some(face) = w.cage.faces.get(i as usize)
                    && let Some(points) = face
                        .iter()
                        .map(|&i| v[i as usize])
                        .collect::<Option<Vec<_>>>()
                {
                    painter.add(egui::Shape::convex_polygon(
                        points,
                        Color32::from_rgba_unmultiplied(255, 175, 45, 58),
                        egui::Stroke::NONE,
                    ));
                }
            }
        }
        for (i, &[a, b]) in w.edges.iter().enumerate() {
            if let (Some(a), Some(b)) = (v[a as usize], v[b as usize]) {
                let picked = p.mode == Mode::Edge && p.items.contains(&(i as u32));
                painter.line_segment(
                    [a, b],
                    if picked {
                        egui::Stroke::new(3., Color32::from_rgb(255, 154, 42))
                    } else {
                        egui::Stroke::new(1.4, Color32::from_rgba_unmultiplied(40, 71, 88, 180))
                    },
                );
            }
        }
        if p.mode == Mode::Vertex {
            for (i, at) in v.iter().enumerate() {
                if let Some(at) = at {
                    let selected = p.items.contains(&(i as u32));
                    painter.circle(
                        *at,
                        if selected { 5. } else { 3.3 },
                        if selected {
                            Color32::from_rgb(255, 159, 53)
                        } else {
                            Color32::from_rgb(215, 236, 239)
                        },
                        egui::Stroke::new(1.3, Color32::from_rgb(49, 78, 90)),
                    );
                }
            }
        }
        if let Some(c) = p.centre() {
            if p.tool == Tool::Rotate {
                if let Some(t) = proj.rings(c) {
                    for (i, ring) in t.rings.into_iter().enumerate() {
                        painter.add(egui::Shape::line(ring, egui::Stroke::new(2.4, INK[i])));
                    }
                }
                return;
            }
            for (i, a, b, _) in proj.handles(c) {
                let ink = egui::Stroke::new(2.4, INK[i]);
                match p.tool {
                    Tool::Move => painter.arrow(a, b - a, ink),
                    Tool::Scale => {
                        painter.line_segment([a, b], ink);
                        painter.rect_filled(
                            egui::Rect::from_center_size(b, Vec2::splat(9.)),
                            1.,
                            INK[i],
                        );
                    }
                    Tool::Rotate => unreachable!(),
                }
                painter.text(
                    b + (b - a).normalized() * 12.,
                    egui::Align2::CENTER_CENTER,
                    ["X", "Y", "Z"][i],
                    egui::FontId::monospace(11.),
                    INK[i],
                );
            }
        }
    }
}
