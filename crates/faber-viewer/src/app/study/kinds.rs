use super::*;
use faber_model::study::ThermalCondition;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Static,
    Modal,
    Thermal,
    Shape,
}

impl Kind {
    pub(super) fn of(k: &StudyKind<Scalar>) -> Self {
        match k {
            StudyKind::Static => Self::Static,
            StudyKind::Modal { .. } => Self::Modal,
            StudyKind::Thermal { .. } => Self::Thermal,
            StudyKind::Shape { .. } => Self::Shape,
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Static => "Static stress",
            Self::Modal => "Frequency",
            Self::Thermal => "Thermal",
            Self::Shape => "Shape optimisation",
        }
    }
    pub(super) fn defaults(self) -> StudyKind<Scalar> {
        match self {
            Self::Static => StudyKind::Static,
            Self::Modal => StudyKind::Modal { modes: 3 },
            Self::Thermal => StudyKind::Thermal {
                conditions: vec![],
                stress: false,
                reference_c: Scalar::Number(20.),
            },
            Self::Shape => StudyKind::Shape {
                target_volume: Scalar::Number(0.5),
                keep: vec![],
                min_member_mm: None,
                symmetry: vec![],
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Heat {
    Temperature,
    Power,
    Convection,
}

impl Heat {
    fn name(self) -> &'static str {
        match self {
            Self::Temperature => "Temperature",
            Self::Power => "Power",
            Self::Convection => "Convection",
        }
    }
    pub(super) fn condition(self, face: FaceSelector<Scalar>) -> ThermalCondition<Scalar> {
        match self {
            Self::Temperature => ThermalCondition::Temperature {
                face,
                celsius: Scalar::Number(20.),
            },
            Self::Power => ThermalCondition::Power {
                face,
                watts: Scalar::Number(5.),
            },
            Self::Convection => ThermalCondition::Convection {
                face,
                w_m2k: Scalar::Number(25.),
                ambient_c: Scalar::Number(20.),
            },
        }
    }
}

pub(super) fn condition_face(c: &mut ThermalCondition<Scalar>) -> &mut FaceSelector<Scalar> {
    match c {
        ThermalCondition::Temperature { face, .. }
        | ThermalCondition::Convection { face, .. }
        | ThermalCondition::Power { face, .. } => face,
    }
}

pub(super) fn setup(
    ui: &mut egui::Ui,
    form: &mut Study<Scalar>,
    data: &ProjectData,
    pick: &mut Option<Target>,
) {
    match &mut form.kind {
        StudyKind::Static => {}
        StudyKind::Modal { modes } => {
            ui.horizontal(|ui| {
                let label = ui.label("Number of modes");
                ui.add(egui::DragValue::new(modes).range(1..=32))
                    .labelled_by(label.id);
            });
        }
        StudyKind::Thermal {
            conditions,
            stress,
            reference_c,
        } => {
            let mut remove = None;
            for (i, c) in conditions.iter_mut().enumerate() {
                ui.push_id(("bc", i), |ui| {
                    let heat = match c {
                        ThermalCondition::Temperature { celsius, .. } => {
                            ui.horizontal(|ui| scalar(ui, "Temperature (°C)", celsius));
                            Heat::Temperature
                        }
                        ThermalCondition::Power { watts, .. } => {
                            ui.horizontal(|ui| scalar(ui, "Total power (W)", watts));
                            Heat::Power
                        }
                        ThermalCondition::Convection {
                            ambient_c, w_m2k, ..
                        } => {
                            ui.horizontal(|ui| scalar(ui, "Air temperature (°C)", ambient_c));
                            ui.horizontal(|ui| scalar(ui, "Film coefficient (W/m² K)", w_m2k));
                            Heat::Convection
                        }
                    };
                    ui.small(face_name(condition_face(c)));
                    ui.horizontal(|ui| {
                        if ui.button("Pick face").clicked() {
                            *pick = Some(Target::Heat(i, heat));
                        }
                        if ui.button("Remove condition").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                conditions.remove(i);
            }
            ui.menu_button("Add heat condition", |ui| {
                for h in [Heat::Temperature, Heat::Power, Heat::Convection] {
                    if ui.button(h.name()).clicked() {
                        *pick = Some(Target::Heat(conditions.len(), h));
                        ui.close();
                    }
                }
            });
            ui.checkbox(stress, "Thermal stress");
            if *stress {
                ui.horizontal(|ui| scalar(ui, "Reference temperature (°C)", reference_c));
            }
        }
        StudyKind::Shape {
            target_volume,
            keep,
            min_member_mm,
            symmetry,
        } => {
            ui.horizontal(|ui| scalar(ui, "Target volume fraction", target_volume));
            let mut automatic = min_member_mm.is_none();
            if ui
                .checkbox(&mut automatic, "Automatic member size")
                .changed()
            {
                *min_member_mm = (!automatic).then_some(Scalar::Number(3.));
            }
            if let Some(v) = min_member_mm {
                ui.horizontal(|ui| scalar(ui, "Minimum member (mm)", v));
            }
            let mut remove = None;
            for (i, f) in keep.iter().enumerate() {
                ui.push_id(("keep", i), |ui| {
                    ui.small(face_name(f));
                    ui.horizontal(|ui| {
                        if ui.button("Pick face").clicked() {
                            *pick = Some(Target::Keep(i));
                        }
                        if ui.button("Remove kept face").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                keep.remove(i);
            }
            if ui.button("Add kept face").clicked() {
                *pick = Some(Target::Keep(keep.len()));
            }
            if let Some(faber_model::Geometry::History { features, .. }) =
                data.parts.get(&form.part).map(|p| &p.geometry)
            {
                let planes = features
                    .iter()
                    .filter(|f| !f.suppressed && crate::app::is_plane(&f.geometry));
                for plane in planes {
                    let mut checked = symmetry.contains(&plane.id);
                    if ui
                        .checkbox(&mut checked, format!("Symmetry · {}", plane.id))
                        .changed()
                    {
                        if checked {
                            symmetry.push(plane.id.clone());
                        } else {
                            symmetry.retain(|id| id != &plane.id);
                        }
                    }
                }
            }
        }
    }
}

fn legend(ui: &mut egui::Ui, heading: &str, range: [f32; 2]) {
    ui.strong(heading);
    ui.horizontal_wrapped(|ui| {
        for i in 0..5 {
            let t = i as f32 * 0.25;
            let [r, g, b] = faber_wiring::study_image::colour(t, 1.);
            ui.vertical(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(28., 7.), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 0., Color32::from_rgb(r, g, b));
                ui.small(format!("{:.1}", range[0] + t * (range[1] - range[0])));
            });
        }
    });
}

impl Shell {
    pub(super) fn paint_other_field(&mut self, field: &StudyField, scale: f64, range: [f32; 2]) {
        let Some(form) = self.study.form.as_ref() else {
            return;
        };
        let Some(index) = self.bodies.iter().position(|b| b.id == form.part) else {
            return;
        };
        match faber_wiring::study_image::display_range(&self.bodies[index], field, scale, range) {
            Ok((shown, ratios)) => self.show_study_body(index, shown, ratios, vec![]),
            Err(e) => self.study.error = Some(e),
        }
    }

    pub(super) fn paint_analysis_field(&mut self) {
        let Some(analysis) = self.study.other.clone() else {
            return;
        };
        let Some(form) = self.study.form.as_ref() else {
            return;
        };
        let r = &analysis.report;
        let (index, range, scale) = match &form.kind {
            StudyKind::Static => return,
            StudyKind::Modal { .. } => {
                let phase = if self.study.animate {
                    (self.study.animation_time * std::f64::consts::TAU * 0.75).sin()
                } else {
                    1.
                };
                (self.study.mode, [0., 1.], self.study.scale * phase)
            }
            StudyKind::Thermal { .. } if self.study.thermal_stress => (
                1,
                [0., r["max_von_mises_mpa"].as_f64().unwrap_or(0.) as f32],
                0.,
            ),
            StudyKind::Thermal { .. } => (
                0,
                [
                    r["min_c"].as_f64().unwrap_or(0.) as f32,
                    r["max_c"].as_f64().unwrap_or(0.) as f32,
                ],
                0.,
            ),
            StudyKind::Shape { .. } => (0, [0., 1.], 0.),
        };
        if let Some(field) = analysis.fields.get(index) {
            self.paint_other_field(field, scale, range);
        }
    }

    pub(super) fn animate_study(&mut self, ctx: &egui::Context) {
        if !self.study.animate {
            return;
        }
        let now = ctx.input(|i| i.time);
        if (now - self.study.animation_time).abs() >= 1. / 30. {
            self.study.animation_time = now;
            self.paint_analysis_field();
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn save_shape_part(&mut self, ctx: &egui::Context, part: String, mesh: Arc<faber_ports::Mesh>) {
        if self.pending.is_some() {
            return;
        }
        let root = self.root.clone();
        let source = self.modified;
        let rev = self.data.revision;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        std::thread::spawn(move || {
            let result = (|| {
                let mut p = faber_wiring::open(&root, false, None)?;
                if faber_wiring::project_fingerprint(&root).ok() != source {
                    return Err("The project changed. Run the study again.".into());
                }
                faber_wiring::study::save_shape(&mut p, &part, &mesh, rev)?;
                Ok((p.data, p.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn study_kind_result(&mut self, ui: &mut egui::Ui) {
        let Some(form) = &self.study.form else {
            return;
        };
        let kind = Kind::of(&form.kind);
        let part = form.part.clone();
        if kind == Kind::Shape
            && let Some(iteration) = self.study.preview_iteration
        {
            ui.small(format!("Density preview · iteration {iteration}"));
            if self.study.stopped {
                ui.label("Stopped at the last preview");
            }
        }
        let Some(analysis) = self.study.other.clone() else {
            if kind == Kind::Shape && self.study.preview_iteration.is_some() {
                ui.add_enabled(false, egui::Button::new("Save as part"));
            }
            return;
        };
        let r = &analysis.report;
        let mut changed = false;
        ui.separator();
        ui.strong("Results");
        match kind {
            Kind::Static => {}
            Kind::Modal => {
                if let Some(modes) = r["modes"].as_array() {
                    for (i, m) in modes.iter().enumerate() {
                        if let Some(hz) = m["frequency_hz"].as_f64() {
                            changed |= ui
                                .selectable_value(
                                    &mut self.study.mode,
                                    i,
                                    format!("Mode {} · {hz:.2} Hz", i + 1),
                                )
                                .changed();
                        }
                    }
                }
                changed |= ui
                    .checkbox(&mut self.study.animate, "Animate mode")
                    .changed();
                ui.horizontal(|ui| {
                    let label = ui.label("Amplitude (mm)");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.study.scale)
                                .range(0.01..=1e5)
                                .speed(0.2),
                        )
                        .labelled_by(label.id)
                        .changed();
                });
            }
            Kind::Thermal => {
                if analysis.fields.len() > 1 {
                    changed |= ui
                        .checkbox(&mut self.study.thermal_stress, "Show thermal stress")
                        .changed();
                }
                if self.study.thermal_stress {
                    legend(
                        ui,
                        "Von Mises stress (MPa)",
                        [0., r["max_von_mises_mpa"].as_f64().unwrap_or(0.) as f32],
                    );
                } else {
                    legend(
                        ui,
                        "Surface temperature (°C)",
                        [
                            r["min_c"].as_f64().unwrap_or(0.) as f32,
                            r["max_c"].as_f64().unwrap_or(0.) as f32,
                        ],
                    );
                }
            }
            Kind::Shape => {
                if let Some(v) = r["volume_share"].as_f64() {
                    ui.label(format!("Volume retained: {:.1} %", 100. * v));
                }
                if let Some(v) = r["mass_saved_g"].as_f64() {
                    ui.label(format!("Estimated mass saved: {v:.2} g"));
                }
                if let Some(v) = r["mesh_mass_g"].as_f64() {
                    ui.label(format!("Mesh mass: {v:.2} g"));
                }
                if let Some(v) = r["mesh_volume_mm3"].as_f64() {
                    ui.label(format!("Mesh volume: {v:.1} mm³"));
                }
                if let Some(v) = r["pieces"].as_u64() {
                    ui.label(format!("Connected pieces: {v}"));
                }
                if let Some(mesh) = &analysis.shape_mesh
                    && ui
                        .add_enabled(
                            self.pending.is_none() && self.study.save_error.is_none(),
                            egui::Button::new("Save as part"),
                        )
                        .clicked()
                {
                    self.save_shape_part(ui.ctx(), part, mesh.clone());
                }
                if let Some(reason) = &self.study.save_error {
                    ui.small(reason);
                }
            }
        }
        if let Some(limits) = r["limits"].as_array() {
            for limit in limits.iter().filter_map(|v| v.as_str()) {
                ui.small(limit);
            }
        }
        if changed {
            self.paint_analysis_field();
        }
    }
}
