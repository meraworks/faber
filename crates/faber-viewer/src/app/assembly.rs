use super::*;
use faber_model::motion::{Joint, JointKind, MotionStudy};
use glam::{DMat4, DVec3, EulerRot, Mat4, Quat, Vec3};

#[derive(Default)]
pub(super) struct Controls {
    relations: super::motion_relations::Editor,
    sweep: Option<mpsc::Receiver<Result<String, String>>>,
    report: Option<String>,
    ranges: BTreeMap<String, [f64; 2]>,
    pub poses: BTreeMap<String, Mat4>,
    values: BTreeMap<String, f64>,
    enabled: bool,
    active: String,
    drag: Option<String>,
    editor: Option<String>,
    notice: Option<String>,
    draft: Option<(String, Joint)>,
}

fn joint_label(id: &str) -> String {
    for (suffix, side) in [("_l", "Left"), ("_r", "Right")] {
        if let Some(name) = id.strip_suffix(suffix) {
            return format!("{side} {}", name.replace('_', " "));
        }
    }
    id.replace('_', " ")
}

fn is_driver(study: &MotionStudy, id: &str) -> bool {
    !study.gears.contains_key(id) && !study.linkages.values().any(|link| link.follower == id)
}

impl Shell {
    fn set_joint_value(&mut self, id: &str, value: f64) {
        let joint = &self.data.motion.joints[id];
        let v = joint.limits.map_or(value, |[lo, hi]| value.clamp(lo, hi));
        let mut values = self.motion.values.clone();
        values.insert(id.into(), v);
        self.apply_motion_values(values);
    }

    pub(crate) fn initial_pose(&mut self, values: BTreeMap<String, f64>) {
        if !values.is_empty() {
            self.motion.enabled = true;
            self.apply_motion_values(values);
        }
    }

    fn apply_motion_values(&mut self, values: BTreeMap<String, f64>) {
        match self.data.motion.solve(&values) {
            Ok(placements) => {
                self.motion.values = values;
                self.motion.poses = placements
                    .into_iter()
                    .map(|(id, p)| {
                        let [x, y, z] = p.rotation.map(|a| a.to_radians() as f32);
                        let r = Quat::from_euler(EulerRot::ZYX, z, y, x);
                        (
                            id,
                            Mat4::from_rotation_translation(
                                r,
                                Vec3::from(p.translation.map(|v| v as f32)),
                            ),
                        )
                    })
                    .collect();
                self.motion.notice = None;
                self.cap_plane = None;
                self.caps = None;
                self.dirty = true;
            }
            Err(e) => self.motion.notice = Some(e),
        }
    }

    pub(super) fn pick_body(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<String> {
        let point = [
            2. * (pos.x - rect.left()) / rect.width() - 1.,
            1. - 2. * (pos.y - rect.top()) / rect.height(),
        ];
        let (eye, ray) = self.camera.ray(rect.aspect_ratio(), point);
        let plane = self.section_on.then_some(self.plane);
        self.bodies
            .iter()
            .filter(|b| !self.hidden.contains(&b.id))
            .filter_map(|b| {
                let m = self
                    .motion
                    .poses
                    .get(&b.id)
                    .map_or(DMat4::IDENTITY, |m| m.as_dmat4());
                let inv = m.inverse();
                let origin = inv.transform_point3(eye);
                let direction = inv.transform_vector3(ray);
                if !faber_scene::ray_hits_bounds(b.bounds, origin, direction) {
                    return None;
                }
                let surface = faber_scene::pick_visible(
                    &b.mesh,
                    origin,
                    direction,
                    plane.map(|p| p.in_body(m)),
                );
                let cap = self
                    .caps
                    .as_ref()
                    .filter(|_| plane.is_some() && plane == self.cap_plane)
                    .and_then(|c| c.cap_mesh(&b.id))
                    .and_then(|mesh| faber_scene::pick(mesh, eye, ray));
                surface
                    .into_iter()
                    .chain(cap)
                    .min_by(f64::total_cmp)
                    .map(|d| (d, &b.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id.clone())
    }

    pub(super) fn select_part_joint(&mut self) {
        if let Some(part) = &self.selected
            && let Some((id, _)) = self
                .data
                .motion
                .joints
                .iter()
                .find(|(id, j)| is_driver(&self.data.motion, id) && j.parts.contains(part))
        {
            self.motion.active.clone_from(id);
        }
    }

    pub(super) fn drag_joint(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        section_handle: bool,
    ) -> bool {
        if !self.motion.enabled || section_handle || !ui.input(|i| i.pointer.primary_down()) {
            self.motion.drag = None;
            return false;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.motion.drag = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|pos| self.pick_body(rect, pos))
                .and_then(|part| {
                    self.data
                        .motion
                        .joints
                        .iter()
                        .find(|(id, j)| is_driver(&self.data.motion, id) && j.parts.contains(&part))
                        .map(|(id, _)| id.clone())
                });
        }
        if let Some(id) = &self.motion.drag {
            self.motion.active.clone_from(id);
        }
        let Some(id) = self.motion.drag.clone() else {
            return false;
        };
        let joint = &self.data.motion.joints[&id];
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let pose = joint
            .parts
            .iter()
            .find_map(|k| self.motion.poses.get(k))
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        let axis = pose
            .transform_vector3(DVec3::from(joint.axis).as_vec3())
            .normalize();
        let origin = pose.transform_point3(DVec3::from(joint.origin).as_vec3());
        let project = |p| {
            let s = matrix.project_point3(p);
            rect.center() + Vec2::new(s.x * rect.width(), -s.y * rect.height()) * 0.5
        };
        let delta = ui.input(|i| i.pointer.delta());
        let change = match joint.kind {
            JointKind::Slider => {
                let d = project(origin + axis) - project(origin);
                if d.length_sq() > 0.01 {
                    delta.dot(d) / d.length_sq()
                } else {
                    -delta.y * self.camera.distance / rect.height()
                }
            }
            JointKind::Revolute => {
                let pos = ui
                    .input(|i| i.pointer.interact_pos())
                    .unwrap_or(rect.center());
                let radial = pos - project(origin);
                let facing = axis.dot((self.camera.eye() - origin).normalize());
                if radial.length() > 6. && facing.abs() > 0.2 {
                    let prev = radial - delta;
                    (prev.y * radial.x - prev.x * radial.y)
                        .atan2(prev.dot(radial))
                        .to_degrees()
                        * facing.signum()
                } else {
                    (delta.x - delta.y) * 0.4
                }
            }
        };
        self.set_joint_value(
            &id,
            self.motion.values.get(&id).copied().unwrap_or(0.) + f64::from(change),
        );
        true
    }

    pub(super) fn motion_controls(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Assembly motion")
            .default_open(!self.data.motion.joints.is_empty())
            .show(ui, |ui| {
                if !self.data.motion.joints.is_empty() {
                    ui.checkbox(&mut self.motion.enabled, "Drag moving parts");
                    let drivers: Vec<_> = self
                        .data
                        .motion
                        .joints
                        .iter()
                        .filter(|(id, _)| is_driver(&self.data.motion, id))
                        .map(|(id, j)| (id.clone(), j.kind, j.limits))
                        .collect();
                    if !drivers.iter().any(|(id, ..)| *id == self.motion.active) {
                        self.motion.active =
                            drivers.first().map(|d| d.0.clone()).unwrap_or_default();
                    }
                    egui::ComboBox::from_label("Joint")
                        .selected_text(joint_label(&self.motion.active))
                        .show_ui(ui, |ui| {
                            for (id, ..) in &drivers {
                                ui.selectable_value(
                                    &mut self.motion.active,
                                    id.clone(),
                                    joint_label(id),
                                );
                            }
                        });
                    if let Some((id, kind, limits)) =
                        drivers.into_iter().find(|d| d.0 == self.motion.active)
                    {
                        let mut v = self.motion.values.get(&id).copied().unwrap_or(0.);
                        let suffix = if kind == JointKind::Slider {
                            " mm"
                        } else {
                            "°"
                        };
                        let changed = if let Some([lo, hi]) = limits {
                            let label = ui.label(if kind == JointKind::Slider {
                                "Distance"
                            } else {
                                "Angle"
                            });
                            ui.add(egui::Slider::new(&mut v, lo..=hi).suffix(suffix))
                                .labelled_by(label.id)
                                .changed()
                        } else {
                            ui.horizontal(|ui| {
                                ui.label(&id);
                                ui.add(egui::DragValue::new(&mut v).suffix(suffix))
                                    .changed()
                            })
                            .inner
                        };
                        if changed {
                            self.set_joint_value(&id, v);
                        }
                        ui.collapsing(format!("Check travel · {id}"), |ui| {
                            let bounds = self
                                .motion
                                .ranges
                                .entry(id.clone())
                                .or_insert(limits.unwrap_or([0., 10.]));
                            ui.horizontal(|ui| {
                                for x in bounds.iter_mut() {
                                    ui.add(egui::DragValue::new(x).suffix(suffix));
                                }
                            });
                            ui.label("Other drives stay at zero.");
                            if ui
                                .add_enabled(
                                    self.motion.sweep.is_none(),
                                    egui::Button::new("Check this range"),
                                )
                                .clicked()
                            {
                                let range = *bounds;
                                let root = self.root.clone();
                                let revision = self.data.revision;
                                let joint = id.clone();
                                let ctx = ui.ctx().clone();
                                let (tx, rx) = mpsc::channel();
                                self.motion.sweep = Some(rx);
                                self.motion.report = None;
                                std::thread::spawn(move || {
                                    let result =
                                        faber_wiring::read(&root).and_then(|(data, bodies)| {
                                            if data.revision != revision {
                                                return Err(
                                                    "The assembly changed. Check the range again"
                                                        .into(),
                                                );
                                            }
                                            let report = faber_wiring::motion_check::sweep(
                                                &bodies,
                                                &data.motion,
                                                &data.contacts,
                                                &joint,
                                                range,
                                                0.,
                                            )?;
                                            Ok(format!(
                                                "{:?} · interval {:.3} to {:.3} · {} poses{}",
                                                report.status,
                                                report.interval[0],
                                                report.interval[1],
                                                report.samples,
                                                report
                                                    .parts
                                                    .map_or(String::new(), |[a, b]| format!(
                                                        " · {a}, {b}"
                                                    ))
                                            ))
                                        });
                                    let _ = tx.send(result);
                                    ctx.request_repaint();
                                });
                            }
                        });
                    }
                    if ui.button("Reset pose").clicked() {
                        self.motion.values.clear();
                        self.motion.poses.clear();
                        self.motion.notice = None;
                        self.cap_plane = None;
                        self.caps = None;
                        self.dirty = true;
                    }
                }
                ui.collapsing("Joint setup", |ui| {
                    if let Some(id) = &self.selected
                        && ui.button("Add joint to selected part").clicked()
                    {
                        let centre = self
                            .bodies
                            .iter()
                            .find(|b| b.id == *id)
                            .map_or([0.; 3], |b| {
                                std::array::from_fn(|i| (b.bounds[0][i] + b.bounds[1][i]) / 2.)
                            });
                        self.motion.draft = Some((
                            format!("{id}_joint"),
                            Joint {
                                kind: JointKind::Revolute,
                                parts: vec![id.clone()],
                                parent: None,
                                origin: centre,
                                axis: [0., 0., 1.],
                                limits: None,
                            },
                        ));
                    }
                    if let Some((id, joint)) = &mut self.motion.draft {
                        ui.text_edit_singleline(id);
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut joint.kind, JointKind::Revolute, "Hinge");
                            ui.selectable_value(&mut joint.kind, JointKind::Slider, "Slider");
                        });
                        for (label, row) in [
                            ("Origin (mm)", &mut joint.origin),
                            ("Axis", &mut joint.axis),
                        ] {
                            ui.label(label);
                            ui.horizontal(|ui| {
                                for v in row {
                                    ui.add(egui::DragValue::new(v).speed(0.1));
                                }
                            });
                        }
                        if ui.button("Save joint").clicked() {
                            let mut study = self.data.motion.clone();
                            if id.trim().is_empty() || study.joints.contains_key(id) {
                                self.motion.notice = Some("Use a new joint name".into());
                            } else {
                                study.joints.insert(id.clone(), joint.clone());
                                self.save_sections(
                                    ui.ctx(),
                                    vec![faber_model::Edit::SetMotion { study }],
                                );
                            }
                        }
                    }
                    if let Some(study) = self.motion.relations.show(ui, &self.data) {
                        self.save_sections(ui.ctx(), vec![faber_model::Edit::SetMotion { study }]);
                    }
                    if ui.button("Edit joint definitions").clicked() {
                        self.motion.editor = serde_json::to_string_pretty(&self.data.motion).ok();
                    }
                    if let Some(text) = &mut self.motion.editor {
                        ui.add(
                            egui::TextEdit::multiline(text)
                                .code_editor()
                                .desired_rows(8),
                        );
                        if ui.button("Apply definitions").clicked() {
                            match serde_json::from_str::<MotionStudy>(text) {
                                Ok(study) => self.save_sections(
                                    ui.ctx(),
                                    vec![faber_model::Edit::SetMotion { study }],
                                ),
                                Err(e) => self.motion.notice = Some(e.to_string()),
                            }
                        }
                    }
                });
                if let Some(rx) = &self.motion.sweep {
                    if let Ok(result) = rx.try_recv() {
                        self.motion.report = Some(result.unwrap_or_else(|e| e));
                        self.motion.sweep = None;
                    } else {
                        ui.spinner();
                    }
                }
                if let Some(report) = &self.motion.report {
                    ui.label(report);
                }
                if let Some(message) = &self.motion.notice {
                    ui.label(message);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn dragging_an_arm_moves_the_linkage_and_keeps_the_camera_fixed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("faber.json"),
            include_bytes!("../../../../examples/assembly-motion/faber.json"),
        )
        .unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let path = dir.path().to_owned();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, path, data, bodies, None));
        h.state_mut().camera.set_view(faber_scene::View::Top);
        h.state_mut().dirty = true;
        for _ in 0..5 {
            h.step();
        }
        h.get_by_label("Drag moving parts").click();
        for _ in 0..3 {
            h.step();
        }
        assert!(h.state().motion.enabled);
        h.get_by_role_and_label(egui::Role::ComboBox, "Joint")
            .click();
        for _ in 0..6 {
            h.step();
        }
        h.get_by_label("input").click();
        h.step();
        assert_eq!(h.state().motion.active, "input");
        for right in [true, false] {
            let before = h.get_by_label("Model viewport").rect();
            let edge = if right {
                before.right_center() + Vec2::X * 5.
            } else {
                before.left_center() - Vec2::X * 5.
            };
            let end = edge + Vec2::new(if right { -48. } else { 48. }, 0.);
            h.hover_at(edge);
            h.step();
            h.drag_at(edge);
            h.step();
            h.hover_at(end);
            for _ in 0..4 {
                h.step();
            }
            h.drop_at(end);
            for _ in 0..3 {
                h.step();
            }
            let after = h.get_by_label("Model viewport").rect();
            assert!(
                before.width() - after.width() > 30.,
                "Panel did not resize: {before:?} to {after:?}"
            );
        }
        let rect = h.get_by_label("Model viewport").rect();
        for angle in [-19.9, -1., 0., 10., 19.9, 0.] {
            h.get_by_role_and_label(egui::Role::Slider, "Angle")
                .set_value_accesskit(angle);
            h.step();
            assert_eq!(
                h.get_by_label("Model viewport").rect(),
                rect,
                "Viewport moved at {angle}"
            );
        }
        let body = h.state().bodies.iter().find(|b| b.id == "input").unwrap();
        let [low, high] = body.bounds.map(DVec3::from);
        let centre = (low + high) * 0.5;
        let p = h
            .state()
            .camera
            .matrix(rect.aspect_ratio())
            .project_point3(centre.as_vec3());
        let pos = rect.center() + Vec2::new(p.x * rect.width(), -p.y * rect.height()) * 0.5;
        assert_eq!(h.state().pick_body(rect, pos).as_deref(), Some("input"));
        let yaw = h.state().camera.yaw;
        h.hover_at(pos);
        h.step();
        h.drag_at(pos);
        h.step();
        let end = pos + Vec2::new(24., 0.);
        h.hover_at(end);
        h.step();
        h.drop_at(end);
        h.step();
        assert_eq!(h.state().camera.yaw, yaw);
        assert!(h.state().motion.values["input"].abs() > 1.);
        for id in ["output", "rod", "wheel"] {
            assert_ne!(h.state().motion.poses[id], Mat4::IDENTITY);
        }
        h.render().unwrap();
    }
}
