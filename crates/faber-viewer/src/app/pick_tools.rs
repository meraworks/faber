use super::{Pick, SelectFilter, Shell};
use faber_core::pick_selector::{PickTopology, edge_loop, face_selector, plane_at, tangent_chain};
use glam::DVec3;
use serde_json::Value;

pub(super) struct PendingPick {
    pub primary: Pick,
    pub double: bool,
    pub near_edge: Option<u32>,
}

enum PickResult {
    Edges(Vec<u32>),
    Field(&'static str, Value),
    Sketch([f64; 3], [f64; 3]),
    None,
}

impl Shell {
    pub(super) fn enter_pick_tool(&mut self) {
        let next = if self.sketch_draft.is_some() {
            SelectFilter::Face
        } else if let Some((_, key)) = self
            .feature_draft
            .as_ref()
            .and_then(|form| form.pick_field())
        {
            if key == "edges" {
                SelectFilter::Edge
            } else {
                SelectFilter::Face
            }
        } else {
            return;
        };
        self.pick_tool_filter.get_or_insert(self.select_filter);
        self.set_filter(next);
        if !self.picks.is_empty() {
            self.handle_pick_tool_click(false, None);
        }
    }

    pub(super) fn handle_pick_tool_click(&mut self, double: bool, near_edge: Option<u32>) {
        if self.saving || (!double && self.pick_tool_filter.is_none()) {
            return;
        }
        let Some(primary) = self.picks.last() else {
            return;
        };
        if matches!(primary, Pick::Body(_)) {
            return;
        }
        let part = primary.part().to_owned();
        self.pending_pick_action = None;
        if !(double && matches!(primary, Pick::Edge { .. }))
            && self.pick_info_result(&part).is_none()
        {
            self.pending_pick_action = Some(PendingPick {
                primary: primary.clone(),
                double,
                near_edge,
            });
            return;
        }
        match self.pick_result(&part, double, near_edge) {
            Ok(PickResult::None) => {}
            Ok(PickResult::Edges(ids)) => {
                self.set_filter(SelectFilter::Edge);
                self.picks = ids
                    .into_iter()
                    .map(|edge| Pick::Edge {
                        part: part.clone(),
                        edge,
                    })
                    .collect();
                self.selected = Some(part);
                self.extra_selected.clear();
                self.dirty = true;
                if self
                    .feature_draft
                    .as_ref()
                    .and_then(|f| f.pick_field())
                    .is_some_and(|(_, key)| key == "edges")
                {
                    self.handle_pick_tool_click(false, None);
                }
            }
            Ok(result) => {
                let update = match result {
                    PickResult::Field(key, value) => self
                        .feature_draft
                        .as_mut()
                        .expect("Open feature form")
                        .set_picked(key, value),
                    PickResult::Sketch(centre, normal) => self
                        .sketch_draft
                        .as_mut()
                        .expect("Open sketch form")
                        .set_face(centre, normal),
                    _ => unreachable!(),
                };
                match update {
                    Ok(()) => {
                        self.window.error = None;
                        self.window.dirty = true;
                    }
                    Err(e) => self.window.error = Some(e),
                }
            }
            Err(e) => self.window.error = Some(e),
        }
    }

    fn pick_result(
        &self,
        part: &str,
        double: bool,
        near_edge: Option<u32>,
    ) -> Result<PickResult, String> {
        let primary = self.picks.last().ok_or("Pick an item.")?;
        let body = self
            .bodies
            .iter()
            .find(|b| b.id == part)
            .ok_or("Select a visible part.")?;
        if double && let Pick::Edge { edge, .. } = primary {
            return Ok(PickResult::Edges(tangent_chain(&body.edge_lines, *edge)));
        }
        if !double && let Some(form) = &self.feature_draft {
            if form.part != part || self.picks.iter().any(|pick| pick.part() != part) {
                return Err(format!("Pick items on {}.", form.part));
            }
            let target = self.data.parts.get(part).ok_or("Select a part.")?;
            faber_core::pick_selector::editable_part(part, target)?;
        }
        if !matches!(primary, Pick::Face { .. } | Pick::Edge { .. }) {
            return Ok(PickResult::None);
        }
        let topo = self
            .pick_info_result(part)
            .ok_or("Reading the selected part.")?
            .as_ref()
            .map_err(Clone::clone)?;
        if double && let (Pick::Face { face, .. }, Some(edge)) = (primary, near_edge) {
            let loop_edges = edge_loop(&body.edge_lines, &topo.links, *face, edge);
            if !loop_edges.is_empty() {
                return Ok(PickResult::Edges(loop_edges));
            }
        }
        if self.sketch_draft.is_some() {
            let (centre, normal) = planar_face(topo, &self.picks)?;
            let pose = topo.inverse.inverse();
            return Ok(PickResult::Sketch(
                pose.transform_point3(centre.into()).into(),
                pose.transform_vector3(normal.into()).normalize().into(),
            ));
        }
        let Some((_, key)) = self.feature_draft.as_ref().and_then(|f| f.pick_field()) else {
            return Ok(PickResult::None);
        };
        let value = if key == "placement" {
            let (_, normal) = planar_face(topo, &self.picks)?;
            let world = self
                .pick_point
                .ok_or("Click the hole position on the face.")?;
            let pose = self.display_pose(part).as_dmat4();
            let local = (topo.inverse * pose.inverse()).transform_point3(DVec3::from(world));
            serde_json::to_value(plane_at(local.into(), normal.map(|v| -v))?)
        } else if key == "edges" {
            let ids = self
                .picks
                .iter()
                .map(|p| match p {
                    Pick::Edge { edge, .. } => Ok(*edge),
                    _ => Err("Pick edges."),
                })
                .collect::<Result<Vec<_>, _>>()?;
            serde_json::to_value(topo.edge_selector(&ids)?)
        } else {
            let ids = self
                .picks
                .iter()
                .map(|p| match p {
                    Pick::Face { face, .. } => Ok(*face),
                    _ => Err("Pick faces."),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if key == "face" && ids.len() != 1 {
                return Err("Pick one face.".into());
            }
            if self
                .feature_draft
                .as_ref()
                .is_some_and(|f| f.requires_planar_pick())
            {
                planar_face(topo, &self.picks)?;
            }
            if self
                .feature_draft
                .as_ref()
                .is_some_and(|f| f.picks_face_list())
            {
                serde_json::to_value(
                    ids.iter()
                        .map(|id| face_selector(&topo.faces, &[*id]))
                        .collect::<Result<Vec<_>, _>>()?,
                )
            } else {
                serde_json::to_value(face_selector(&topo.faces, &ids)?)
            }
        }
        .map_err(|e| e.to_string())?;
        Ok(PickResult::Field(key, value))
    }
}

fn planar_face(topo: &PickTopology, picks: &[Pick]) -> Result<([f64; 3], [f64; 3]), String> {
    let [Pick::Face { face, .. }] = picks else {
        return Err("Pick one planar face.".into());
    };
    let f = topo
        .faces
        .iter()
        .find(|f| f.index == *face)
        .ok_or("Pick a face of the part.")?;
    Ok((f.centre, f.normal.ok_or("Pick one planar face.")?))
}
