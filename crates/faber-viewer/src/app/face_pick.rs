use super::*;
use faber_core::pick_selector::PickTopology;
use glam::{DMat4, DVec3};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SelectFilter {
    #[default]
    Body,
    Face,
    Edge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Body(String),
    Face { part: String, face: u32 },
    Edge { part: String, edge: u32 },
}

impl Pick {
    pub(super) fn part(&self) -> &str {
        match self {
            Self::Body(id) | Self::Face { part: id, .. } | Self::Edge { part: id, .. } => id,
        }
    }
    pub(super) fn face_key(&self) -> Option<(String, u32)> {
        if let Self::Face { part, face } = self {
            Some((part.clone(), *face))
        } else {
            None
        }
    }
}

#[derive(Default)]
pub(super) struct InfoCache {
    ready: BTreeMap<String, Result<PickTopology, String>>,
    pending: Option<(String, mpsc::Receiver<Result<PickTopology, String>>)>,
}

impl Shell {
    pub(super) fn section_for_body(&self, id: &str) -> Option<Section> {
        (self.section_on && (self.section_scope.is_empty() || self.section_scope.contains(id)))
            .then_some(self.plane)
    }
    pub(super) fn set_filter(&mut self, filter: SelectFilter) {
        if self.select_filter == filter {
            return;
        }
        self.select_filter = filter;
        self.clear_entity_picks();
    }

    pub(super) fn clear_entity_picks(&mut self) {
        self.pending_pick_action = None;
        self.picks.clear();
        self.pick_point = None;
        self.hover_pos = None;
        self.hover_pick = None;
        self.hover_body = None;
        self.gpu.hovered = None;
        self.dirty = true;
    }

    fn pick_pose(&self, part: &str) -> DMat4 {
        self.motion
            .poses
            .get(part)
            .map_or(DMat4::IDENTITY, |m| m.as_dmat4())
    }

    fn front_triangle(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<(&Body, usize, DVec3)> {
        let ndc = [
            2. * (pos.x - rect.left()) / rect.width() - 1.,
            1. - 2. * (pos.y - rect.top()) / rect.height(),
        ];
        let (eye, ray) = self.camera.ray(rect.aspect_ratio(), ndc);
        self.bodies
            .iter()
            .filter(|b| !self.hidden.contains(&b.id))
            .filter_map(|body| {
                let pose = self.pick_pose(&body.id);
                let inverse = pose.inverse();
                let origin = inverse.transform_point3(eye);
                let direction = inverse.transform_vector3(ray);
                if !faber_scene::ray_hits_bounds(body.bounds, origin, direction) {
                    return None;
                }
                let cut = self.section_for_body(&body.id).map(|p| p.in_body(pose));
                let (distance, triangle) =
                    faber_scene::pick_triangle(&body.mesh, origin, direction, cut)?;
                Some((body, triangle, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(body, triangle, t)| (body, triangle, eye + t * ray))
    }

    fn edge_at(
        &self,
        body: &Body,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<faber_scene::EdgeHit> {
        let matrix = self.camera.matrix(rect.aspect_ratio());
        faber_scene::pick_edge_hit_where(
            &body.edge_lines,
            self.pick_pose(&body.id),
            matrix,
            [rect.min.into(), rect.max.into()],
            pos.into(),
            8.,
            self.section_for_body(&body.id),
            |hit| {
                let point = DVec3::from(hit.point);
                let Some(at) = move_tool::screen_point(rect, matrix, point) else {
                    return false;
                };
                let Some((_, _, front)) = self.front_triangle(rect, at) else {
                    return true;
                };
                // Allow for rounding on a silhouette seen edge-on.
                let eye = self.camera.eye().as_dvec3();
                point.distance(eye) <= front.distance(eye) + f64::from(self.camera.distance) * 2e-5
            },
        )
    }

    pub(super) fn pick_entity(
        &self,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<(Pick, [f64; 3])> {
        if self.select_filter == SelectFilter::Body {
            return self
                .pick_surface(rect, pos)
                .map(|(id, p)| (Pick::Body(id), p.into()));
        }
        if self.select_filter == SelectFilter::Edge {
            let nearest = self
                .bodies
                .iter()
                .filter(|b| !self.hidden.contains(&b.id))
                .filter_map(|b| self.edge_at(b, rect, pos).map(|hit| (&b.id, hit)))
                .min_by(|a, b| {
                    a.1.distance_px
                        .total_cmp(&b.1.distance_px)
                        .then(a.1.depth.total_cmp(&b.1.depth))
                });
            if let Some((part, hit)) = nearest {
                return Some((
                    Pick::Edge {
                        part: part.clone(),
                        edge: hit.edge,
                    },
                    hit.point,
                ));
            }
        }
        let (body, triangle, p) = self.front_triangle(rect, pos)?;
        match body.face_of_triangle.get(triangle) {
            None => Some((Pick::Body(body.id.clone()), p.into())),
            Some(&face) if self.select_filter == SelectFilter::Face => Some((
                Pick::Face {
                    part: body.id.clone(),
                    face,
                },
                p.into(),
            )),
            _ => None,
        }
    }

    pub(super) fn pick_canvas(
        &mut self,
        rect: egui::Rect,
        pos: egui::Pos2,
        ctrl: bool,
        shift: bool,
        double: bool,
        secondary: bool,
    ) {
        let picked = if secondary && self.select_filter == SelectFilter::Body {
            // Retain the exact face under the cursor from the same ray used to select the body.
            let hit = self.pick_surface_face(rect, pos);
            self.marking.face = hit.as_ref().and_then(|(_, _, face)| *face);
            hit.map(|(part, point, _)| (Pick::Body(part), point.into()))
        } else {
            if secondary {
                self.marking.face = None;
            }
            self.pick_entity(rect, pos)
        };
        if self.select_filter == SelectFilter::Body {
            let body = picked.as_ref().map(|(pick, _)| pick.part().to_owned());
            if secondary {
                self.select_menu_target(body);
            } else {
                self.select_picked(body, ctrl, shift);
            }
            self.picks = self
                .extra_selected
                .iter()
                .chain(self.selected.iter())
                .cloned()
                .map(Pick::Body)
                .collect();
            self.pick_point = picked
                .filter(|(pick, _)| Some(pick.part()) == self.selected.as_deref())
                .map(|(_, world)| world);
            return;
        }
        let Some((pick, world)) = picked else {
            if !ctrl && !shift {
                self.select_only(None);
            }
            return;
        };
        let member = self.picks.iter().position(|p| p == &pick);
        if !(ctrl || shift || secondary && member.is_some()) {
            self.picks.clear();
        }
        self.picks.retain(|p| p != &pick);
        if !ctrl || member.is_none() {
            self.picks.push(pick.clone());
        }
        self.selected = self.picks.last().map(|p| p.part().to_owned());
        self.extra_selected = self
            .picks
            .iter()
            .map(|p| p.part().to_owned())
            .filter(|p| Some(p) != self.selected.as_ref())
            .collect();
        self.pick_point = if self.picks.last() == Some(&pick) {
            Some(world)
        } else {
            None
        };
        self.dirty = true;
        if !secondary {
            let near_edge = (double && self.select_filter == SelectFilter::Face)
                .then(|| {
                    self.bodies
                        .iter()
                        .find(|b| b.id == pick.part())
                        .and_then(|b| self.edge_at(b, rect, pos))
                        .map(|h| h.edge)
                })
                .flatten();
            self.handle_pick_tool_click(double, near_edge);
        }
    }

    pub(super) fn hover_entity(&mut self, rect: egui::Rect, pos: Option<egui::Pos2>) {
        let next = pos
            .and_then(|p| self.pick_entity(rect, p))
            .map(|(pick, _)| pick);
        let body = next.as_ref().map(|p| p.part().to_owned()).or_else(|| {
            if self.select_filter == SelectFilter::Edge {
                pos.and_then(|p| self.pick_body(rect, p))
            } else {
                None
            }
        });
        self.gpu.hovered = match &next {
            Some(Pick::Body(id)) => Some(id.clone()),
            _ => None,
        };
        self.dirty |= self.hover_pick != next || self.hover_body != body;
        self.hover_pick = next;
        self.hover_body = body;
        self.hover_pos = pos;
    }

    pub(super) fn cached_pick_topology(&self, part: &str) -> Option<&PickTopology> {
        self.pick_info.ready.get(part)?.as_ref().ok()
    }

    pub(super) fn pick_info_result(&self, part: &str) -> Option<&Result<PickTopology, String>> {
        self.pick_info.ready.get(part)
    }

    pub(super) fn poll_pick_info(&mut self, ctx: &egui::Context) {
        if let Some((id, rx)) = &self.pick_info.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pick_info.ready.insert(id.clone(), result);
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pick_info
                        .ready
                        .insert(id.clone(), Err("Cannot read the part faces.".into()));
                }
            }
            self.pick_info.pending = None;
            if self
                .pending_pick_action
                .as_ref()
                .is_some_and(|action| self.pick_info.ready.contains_key(action.primary.part()))
                && let Some(action) = self.pending_pick_action.take()
                && self.picks.last() == Some(&action.primary)
            {
                self.handle_pick_tool_click(action.double, action.near_edge);
                ctx.request_repaint();
            }
        }
        let Some(part) = self
            .picks
            .iter()
            .find_map(|p| match p {
                Pick::Face { part, .. } | Pick::Edge { part, .. }
                    if !self.pick_info.ready.contains_key(part) =>
                {
                    Some(part.clone())
                }
                _ => None,
            })
            .or_else(|| {
                self.context_face()
                    .map(|(id, _)| id)
                    .filter(|id| !self.pick_info.ready.contains_key(*id))
                    .map(str::to_owned)
            })
        else {
            return;
        };
        let bodies = self.bodies.clone();
        let data = self.data.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pick_info.pending = Some((part.clone(), rx));
        std::thread::spawn(move || {
            let row = bodies
                .iter()
                .find(|b| b.id == part)
                .ok_or_else(|| "Part no longer exists.".to_string())
                .and_then(|body| faber_wiring::pick_topology(body, &data));
            let _ = tx.send(row);
            ctx.request_repaint();
        });
    }

    pub(super) fn pick_inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, filter) in [
                ("Body", SelectFilter::Body),
                ("Face", SelectFilter::Face),
                ("Edge", SelectFilter::Edge),
            ] {
                if ui
                    .selectable_label(self.select_filter == filter, label)
                    .on_hover_text(format!("Select a {}", label.to_lowercase()))
                    .clicked()
                {
                    self.set_filter(filter);
                }
            }
        });
        for pick in &self.picks {
            let part = pick.part();
            let topology = self.cached_pick_topology(part);
            match pick {
                Pick::Face { face, .. } => {
                    ui.label(format!("Face {face} of {part}"));
                    if let Some(f) =
                        topology.and_then(|t| t.faces.iter().find(|f| f.index == *face))
                    {
                        let kind = if f.normal.is_some() {
                            "Plane"
                        } else if f.cylinder.is_some() {
                            "Cylinder"
                        } else {
                            "Curved surface"
                        };
                        ui.small(format!("{kind} · {:.2} mm²", f.area));
                    }
                }
                Pick::Edge { edge, .. } => {
                    ui.label(format!("Edge {edge} of {part}"));
                    if let Some(e) =
                        topology.and_then(|t| t.edges.iter().find(|e| e.index == *edge))
                    {
                        ui.small(format!("{:.2} mm", e.length));
                    }
                }
                Pick::Body(_) => {}
            }
            if let Some(Err(message)) = self.pick_info.ready.get(part) {
                ui.small(message);
            }
        }
    }

    pub(super) fn draw_pick_edges(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.select_filter != SelectFilter::Edge
            && !self.picks.iter().any(|p| matches!(p, Pick::Edge { .. }))
        {
            return;
        }
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let painter = ui.painter().with_clip_rect(rect);
        for body in self.bodies.iter().filter(|b| !self.hidden.contains(&b.id)) {
            let all = self.select_filter == SelectFilter::Edge
                && self.hover_body.as_deref() == Some(&body.id);
            let selected = self
                .picks
                .iter()
                .filter_map(|p| match p {
                    Pick::Edge { part, edge } if *part == body.id => Some(*edge),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            if !all && selected.is_empty() {
                continue;
            }
            let transform = self.pick_pose(&body.id);
            for (i, range) in body.edge_lines.ranges.iter().enumerate() {
                let edge = (i + 1) as u32;
                let active = selected.contains(&edge)
                    || matches!(&self.hover_pick,Some(Pick::Edge{part,edge:e}) if part==&body.id && *e==edge);
                if !all && !active {
                    continue;
                }
                let stroke = if active {
                    egui::Stroke::new(3.0, Color32::from_rgb(42, 176, 207))
                } else {
                    egui::Stroke::new(1., Color32::from_black_alpha(88))
                };
                for segment in
                    body.edge_lines.points[range.start as usize..range.end as usize].windows(2)
                {
                    let mut a = transform.transform_point3(segment[0].into());
                    let mut b = transform.transform_point3(segment[1].into());
                    if let Some(plane) = self.section_for_body(&body.id) {
                        let n = DVec3::from(plane.normal).normalize();
                        let sign = if plane.reverse { -1. } else { 1. };
                        let da = sign * (n.dot(a) - plane.offset);
                        let db = sign * (n.dot(b) - plane.offset);
                        if da > 0. && db > 0. {
                            continue;
                        }
                        if (da > 0.) != (db > 0.) {
                            let cut = a.lerp(b, da / (da - db));
                            if da > 0. {
                                a = cut;
                            } else {
                                b = cut;
                            }
                        }
                    }
                    if let (Some(a), Some(b)) = (
                        move_tool::screen_point(rect, matrix, a),
                        move_tool::screen_point(rect, matrix, b),
                    ) {
                        painter.line_segment([a, b], stroke);
                    }
                }
            }
        }
    }
}
