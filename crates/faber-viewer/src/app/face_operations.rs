use super::*;
use faber_model::{EmbossMode, FaceSelector, Geometry, Scalar};

#[derive(Clone, Copy)]
enum FaceTool {
    Remove,
    Move,
    Split,
    Emboss,
}

impl Shell {
    pub(super) fn context_face(&self) -> Option<(&str, u32)> {
        if self.select_filter != SelectFilter::Body
            || self.marking.anchor.is_none()
            || !self.extra_selected.is_empty()
        {
            return None;
        }
        Some((self.selected.as_deref()?, self.marking.face?))
    }
    pub(super) fn face_menu(&mut self, ui: &mut egui::Ui) {
        let automatic = self.context_face().map(|(id, face)| (id.to_owned(), face));
        let part = match (&automatic, self.picks.last()) {
            (Some((id, _)), _) => id.clone(),
            (None, Some(Pick::Face { part, .. })) if self.select_filter == SelectFilter::Face => {
                part.clone()
            }
            _ => return,
        };
        let body = self.bodies.iter().find(|b| b.id == part);
        let reason = if automatic.is_none()
            && self
                .picks
                .iter()
                .any(|p| !matches!(p,Pick::Face{part:id,..} if id==&part))
        {
            Some("Pick faces on one part.".to_owned())
        } else if body.is_none_or(|b| b.shape.is_none()) {
            Some(format!(
                "Part {part} is a mesh. Convert it to a solid first."
            ))
        } else {
            faber_core::pick_selector::editable_part(&part, &self.data.parts[&part]).err()
        };
        let ready = self.pick_info_result(&part);
        let reading = match ready {
            None => Some("Reading the selected face.".into()),
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        };
        for (label, tool) in [
            ("Remove face", FaceTool::Remove),
            ("Move face", FaceTool::Move),
            ("Split face", FaceTool::Split),
            ("Emboss", FaceTool::Emboss),
        ] {
            let single = matches!(tool, FaceTool::Split | FaceTool::Emboss);
            let disabled = reason
                .as_deref()
                .or({
                    if matches!(tool, FaceTool::Move) {
                        None
                    } else {
                        reading.as_deref()
                    }
                })
                .or_else(|| (single && self.picks.len() != 1).then_some("Pick one face."));
            let response = ui.add_enabled(
                disabled.is_none() && self.pending.is_none() && self.press.is_none(),
                egui::Button::new(label),
            );
            if let Some(error) = disabled {
                response.on_disabled_hover_text(error);
                continue;
            }
            if response.clicked() {
                if let Some((id, face)) = &automatic {
                    let point = self.pick_point;
                    self.set_filter(SelectFilter::Face);
                    self.picks.push(Pick::Face {
                        part: id.clone(),
                        face: *face,
                    });
                    self.pick_point = point;
                }
                if matches!(tool, FaceTool::Move) {
                    self.start_face_drag(ui.ctx(), true);
                } else if let Err(error) = self.face_form(&part, tool) {
                    self.error = Some(error);
                }
                ui.close();
                break;
            }
        }
        ui.separator();
    }

    fn face_form(&mut self, id: &str, tool: FaceTool) -> Result<(), String> {
        let topo = self
            .cached_pick_topology(id)
            .ok_or("Reading the selected face.")?;
        let faces: Vec<FaceSelector<Scalar>> = self
            .picks
            .iter()
            .map(|p| match p {
                Pick::Face { part, face } if part == id => {
                    faber_core::pick_selector::face_selector(&topo.faces, &[*face])
                }
                _ => Err("Pick faces on one part.".into()),
            })
            .collect::<Result<_, _>>()?;
        let part = &self.data.parts[id];
        let (features, output) = match &part.geometry {
            Geometry::History { features, output } => (features.as_slice(), output.as_str()),
            _ => (&[][..], "source"),
        };
        let body = output.to_owned();
        let (prefix, geometry) = match tool {
            FaceTool::Remove => ("remove_faces", Geometry::RemoveFaces { body, faces }),
            FaceTool::Split => {
                let face = faces.into_iter().next().ok_or("Pick one face.")?;
                let (tool, _) = crate::feature_form::split_tools(&part.geometry, "")
                    .into_iter()
                    .next()
                    .ok_or("Add a reference plane or a sketch to the part first.")?;
                ("split_face", Geometry::SplitFace { body, face, tool })
            }
            FaceTool::Emboss => {
                let face = faces.into_iter().next().ok_or("Pick one face.")?;
                let sketch = crate::feature_form::sketch_ids(&part.geometry, "")
                    .into_iter()
                    .next()
                    .ok_or("Add a sketch for the emboss profile first.")?;
                let Pick::Face { face: index, .. } = self.picks.last().unwrap() else {
                    unreachable!()
                };
                let f = topo
                    .faces
                    .iter()
                    .find(|f| f.index == *index)
                    .ok_or("Pick one face.")?;
                if f.normal.is_none() && f.cylinder.is_none() {
                    return Err("Emboss needs a flat or a cylindrical face.".into());
                }
                let at = f.cylinder.as_ref().map(|_| [0.0.into(), 0.0.into()]);
                (
                    "emboss",
                    Geometry::Emboss {
                        body,
                        face,
                        sketch,
                        depth: 1.0.into(),
                        mode: EmbossMode::Raise,
                        at,
                    },
                )
            }
            FaceTool::Move => return Err("Use the face handle.".into()),
        };
        let fid = (1..)
            .map(|n| format!("{prefix}_{n}"))
            .find(|name| !features.iter().any(|f| f.id.eq_ignore_ascii_case(name)))
            .unwrap();
        let mut form =
            crate::feature_form::FeatureForm::new(self.data.clone(), id.into(), fid, &geometry)?;
        form.create = true;
        self.request_window(Draft::Feature(form));
        self.error = None;
        Ok(())
    }
}
