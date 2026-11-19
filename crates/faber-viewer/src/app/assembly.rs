use super::*;
use faber_model::motion::{Joint, JointKind, MotionStudy, Pose, PoseValue};
use glam::{DMat4, DVec3, EulerRot, Mat4, Quat, Vec3};

#[derive(Default)]
pub(super) struct Controls {
    relations: super::motion_relations::Editor,
    sweep: Option<mpsc::Receiver<Result<String, String>>>,
    report: Option<String>,
    ranges: BTreeMap<(String, usize), [f64; 2]>,
    pub poses: BTreeMap<String, Mat4>,
    values: Pose,
    enabled: bool,
    active: String,
    coordinate: usize,
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
    study.joints[id].kind.count() > 0
        && !study.gears.contains_key(id)
        && !study.linkages.values().any(|link| link.follower == id)
}
fn driver_for_part<'a>(study: &'a MotionStudy, part: &str) -> Option<&'a String> {
    let (mut id, mut j) = study
        .joints
        .iter()
        .find(|(_, j)| j.parts.iter().any(|p| p == part))?;
    while j.kind == JointKind::Rigid {
        (id, j) = study.joints.get_key_value(j.parent.as_ref()?)?;
    }
    is_driver(study, id).then_some(id)
}
pub(super) fn names(kind: JointKind) -> &'static [&'static str] {
    match kind {
        JointKind::Rigid => &[],
        JointKind::Revolute => &["Angle"],
        JointKind::Slider => &["Distance"],
        JointKind::Cylindrical | JointKind::PinSlot => &["Angle", "Distance"],
        JointKind::Planar => &["X distance", "Y distance", "Angle"],
        JointKind::Ball => &["Rotation X", "Rotation Y", "Rotation Z"],
    }
}
fn coordinate_limit(j: &Joint, i: usize) -> Option<[f64; 2]> {
    if j.kind == JointKind::Ball && i == 0 {
        None
    } else {
        j.range(i)
    }
}

impl Shell {
    fn set_joint_value(&mut self, id: &str, i: usize, value: f64) {
        let joint = &self.data.motion.joints[id];
        let v = coordinate_limit(joint, i).map_or(value, |[lo, hi]| value.clamp(lo, hi));
        let mut values = self.motion.values.clone();
        let mut coords = values
            .get(id)
            .map(|p| p.as_slice().to_vec())
            .unwrap_or_default();
        coords.resize(joint.kind.count(), 0.);
        coords[i] = v;
        values.insert(id.into(), PoseValue::from_values(coords));
        self.motion.coordinate = i;
        self.apply_motion_values(values);
    }

    pub(crate) fn initial_pose(&mut self, values: Pose) {
        if !values.is_empty() {
            self.motion.enabled = true;
            self.apply_motion_values(values);
        }
    }

    fn apply_motion_values(&mut self, values: Pose) {
        match self.data.motion.solve_pose(&values) {
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
        self.pick_surface(rect, pos).map(|(id, _)| id)
    }

    pub(super) fn pick_surface(
        &self,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<(String, DVec3)> {
        self.pick_surface_face(rect, pos)
            .map(|(id, point, _)| (id, point))
    }

    pub(super) fn pick_surface_face(
        &self,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<(String, DVec3, Option<u32>)> {
        let point = [
            2. * (pos.x - rect.left()) / rect.width() - 1.,
            1. - 2. * (pos.y - rect.top()) / rect.height(),
        ];
        let (eye, ray) = self.camera.ray(rect.aspect_ratio(), point);
        let poses = self.move_poses();
        self.bodies
            .iter()
            .map(|b| self.intermediate_body(b))
            .filter(|b| !self.hidden.contains(&b.id))
            .filter_map(|b| {
                let pose = poses.get(&b.id).map_or(DMat4::IDENTITY, |m| m.as_dmat4());
                self.surface_hit(b, pose, eye, ray)
                    .map(|(depth, face)| (depth, &b.id, face))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(d, id, face)| (id.clone(), eye + d * ray, face))
    }

    pub(super) fn surface_depth(
        &self,
        body: &Body,
        pose: DMat4,
        eye: DVec3,
        ray: DVec3,
    ) -> Option<f64> {
        self.surface_hit(body, pose, eye, ray)
            .map(|(depth, _)| depth)
    }

    fn surface_hit(
        &self,
        body: &Body,
        pose: DMat4,
        eye: DVec3,
        ray: DVec3,
    ) -> Option<(f64, Option<u32>)> {
        let inverse = pose.inverse();
        let origin = inverse.transform_point3(eye);
        let direction = inverse.transform_vector3(ray);
        if !faber_scene::ray_hits_bounds(body.bounds, origin, direction) {
            return None;
        }
        let plane = self.section_for_body(&body.id);
        let surface = faber_scene::pick_triangle(
            &body.mesh,
            origin,
            direction,
            plane.map(|p| p.in_body(pose)),
        );
        let cap = self
            .caps
            .as_ref()
            .filter(|_| {
                !self.replacing_solid(&body.id)
                    && plane.is_some()
                    && plane == self.cap_plane
                    && self
                        .cap_poses
                        .get(&body.id)
                        .map_or(DMat4::IDENTITY, |m| m.as_dmat4())
                        == pose
            })
            .and_then(|c| c.cap_mesh(&body.id))
            .and_then(|mesh| faber_scene::pick(mesh, eye, ray));
        surface
            .map(|(depth, triangle)| (depth, body.face_of_triangle.get(triangle).copied()))
            .into_iter()
            .chain(cap.map(|depth| (depth, None)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    pub(super) fn select_part_joint(&mut self) {
        if let Some(part) = &self.selected
            && let Some(id) = driver_for_part(&self.data.motion, part)
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
        if !self.motion.enabled
            || self.picking_surface_edge()
            || section_handle
            || !ui.input(|i| i.pointer.primary_down())
        {
            self.motion.drag = None;
            return false;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.motion.drag = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|pos| self.pick_body(rect, pos))
                .and_then(|part| driver_for_part(&self.data.motion, &part).cloned());
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
        let i = self.motion.coordinate.min(joint.kind.count() - 1);
        let n = DVec3::from(joint.axis).normalize();
        let axis = match (joint.kind, i) {
            (JointKind::PinSlot, 1) | (JointKind::Planar, 0) => {
                DVec3::from(joint.slide_axis().unwrap_or([1., 0., 0.]))
            }
            (JointKind::Planar, 1) => n.cross(joint.slide_axis().unwrap_or([1., 0., 0.]).into()),
            (JointKind::Ball, 0) => DVec3::X,
            (JointKind::Ball, 1) => DVec3::Y,
            (JointKind::Ball, 2) => DVec3::Z,
            _ => n,
        };
        let parent = joint
            .parent
            .as_ref()
            .and_then(|parent| self.data.motion.joints.get(parent))
            .and_then(|j| j.parts.iter().find_map(|p| self.motion.poses.get(p)))
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        let axis = parent.transform_vector3(axis.as_vec3()).normalize();
        let origin = pose.transform_point3(DVec3::from(joint.origin).as_vec3());
        let project = |p| {
            let s = matrix.project_point3(p);
            rect.center() + Vec2::new(s.x * rect.width(), -s.y * rect.height()) * 0.5
        };
        let delta = ui.input(|i| i.pointer.delta());
        let change = if !joint.kind.angular(i) {
            let d = project(origin + axis) - project(origin);
            if d.length_sq() > 0.01 {
                delta.dot(d) / d.length_sq()
            } else {
                -delta.y * self.camera.distance / rect.height()
            }
        } else {
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
        };
        self.set_joint_value(
            &id,
            i,
            self.motion
                .values
                .get(&id)
                .and_then(|v| v.as_slice().get(i))
                .copied()
                .unwrap_or(0.)
                + f64::from(change),
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
                        .map(|(id, j)| (id.clone(), j.clone()))
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
                    if let Some((id, joint)) =
                        drivers.into_iter().find(|d| d.0 == self.motion.active)
                    {
                        for (i, &name) in names(joint.kind).iter().enumerate() {
                            let mut value = self
                                .motion
                                .values
                                .get(&id)
                                .and_then(|v| v.as_slice().get(i))
                                .copied()
                                .unwrap_or(0.);
                            let unit = if joint.kind.angular(i) { "°" } else { " mm" };
                            let label = ui.label(name);
                            let limit = coordinate_limit(&joint, i);
                            let [lo, hi] = limit.unwrap_or(if joint.kind.angular(i) {
                                [-180., 180.]
                            } else {
                                [-100., 100.]
                            });
                            let mut slider = egui::Slider::new(&mut value, lo..=hi).suffix(unit);
                            if limit.is_none() {
                                slider = slider.clamping(egui::SliderClamping::Never);
                            }
                            let response = ui.add(slider).labelled_by(label.id);
                            if response.changed() {
                                self.set_joint_value(&id, i, value);
                            }
                            if response.clicked() || response.drag_started() {
                                self.motion.coordinate = i;
                            }
                        }
                        let i = self.motion.coordinate.min(joint.kind.count() - 1);
                        let limits = coordinate_limit(&joint, i);
                        let suffix = if joint.kind.angular(i) { "°" } else { " mm" };
                        ui.collapsing(format!("Check travel · {id}"), |ui| {
                            let bounds = self
                                .motion
                                .ranges
                                .entry((id.clone(), i))
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
                                            let report =
                                                faber_wiring::motion_check::sweep_coordinate(
                                                    &bodies,
                                                    &data.motion,
                                                    &data.contacts,
                                                    &joint,
                                                    i,
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
                                second_axis: None,
                                limits2: None,
                                limits3: None,
                            },
                        ));
                    }
                    if let Some((id, joint)) = &mut self.motion.draft {
                        ui.text_edit_singleline(id);
                        super::motion_relations::choose_kind(ui, joint);
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
                        super::motion_relations::secondary(ui, joint);
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

    fn copy_folder(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_folder(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn cylindrical_joint_sliders_preserve_both_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        copy_folder(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/assembly-motion"),
            dir.path(),
        );
        let (mut data, bodies) = faber_wiring::read(dir.path()).unwrap();
        data.motion=serde_json::from_value(serde_json::json!({"joints":{"piston":{"kind":"cylindrical","parts":["input"],"origin":[0,0,0],"axis":[0,0,1],"limits":[-90,90],"limits2":[-10,10]}}})).unwrap();
        let path = dir.path().to_owned();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, path, data, bodies, None));
        for _ in 0..5 {
            h.step();
        }
        h.get_by_role_and_label(egui::Role::Slider, "Angle")
            .set_value_accesskit(90.);
        h.step();
        h.get_by_role_and_label(egui::Role::Slider, "Distance")
            .set_value_accesskit(5.);
        h.step();
        assert!(
            h.state().motion.poses["input"]
                .transform_point3(Vec3::X * 10.)
                .distance(Vec3::new(0., 10., 5.))
                < 1e-5
        );
    }
    #[test]
    fn dragging_an_arm_moves_the_linkage_and_keeps_the_camera_fixed() {
        let dir = tempfile::tempdir().unwrap();
        copy_folder(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/assembly-motion"),
            dir.path(),
        );
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
        assert!(h.state().motion.values["input"].as_slice()[0].abs() > 1.);
        for id in ["output", "rod", "wheel"] {
            assert_ne!(h.state().motion.poses[id], Mat4::IDENTITY);
        }
        h.render().unwrap();
    }
}
