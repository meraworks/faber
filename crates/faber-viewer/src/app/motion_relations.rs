use eframe::egui;
use faber_model::{
    ProjectData,
    motion::{FourBar, Gear, JointKind, MotionStudy},
};

#[derive(Default)]
pub(super) struct Editor {
    draft: Option<MotionStudy>,
    driver: String,
    follower: String,
    linkage_name: String,
    error: Option<String>,
}

impl Editor {
    pub(super) fn show(&mut self, ui: &mut egui::Ui, data: &ProjectData) -> Option<MotionStudy> {
        if self.draft.is_none() {
            if ui.button("Edit joints and relations").clicked() {
                self.draft = Some(data.motion.clone());
            }
            return None;
        }
        let draft = self.draft.as_mut()?;
        let ids: Vec<_> = draft.joints.keys().cloned().collect();
        let hinges: Vec<_> = draft
            .joints
            .iter()
            .filter(|(_, j)| j.kind == JointKind::Revolute)
            .map(|(id, _)| id.clone())
            .collect();
        for (id, joint) in &mut draft.joints {
            ui.push_id(("motion joint", id), |ui| {
                ui.collapsing(format!("Joint {id}"), |ui| {
                    let mut parent = joint.parent.clone().unwrap_or_default();
                    let parents = std::iter::once(String::new())
                        .chain(ids.iter().filter(|other| *other != id).cloned())
                        .collect::<Vec<_>>();
                    choose(ui, "Parent", &mut parent, &parents);
                    joint.parent = (!parent.is_empty()).then_some(parent);
                    coordinates(ui, "Origin (mm)", &mut joint.origin);
                    coordinates(ui, "Axis", &mut joint.axis);
                    let mut limited = joint.limits.is_some();
                    if ui.checkbox(&mut limited, "Limit travel").changed() {
                        joint.limits = limited.then_some([-1.0, 1.0]);
                    }
                    if let Some([low, high]) = &mut joint.limits {
                        let unit = if joint.kind == JointKind::Slider {
                            " mm"
                        } else {
                            "°"
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Minimum");
                            ui.add(egui::DragValue::new(low).suffix(unit));
                            ui.label("Maximum");
                            ui.add(egui::DragValue::new(high).suffix(unit));
                        });
                    }
                    part_choices(ui, data, &mut joint.parts);
                });
            });
        }
        draft.gears.retain(|id, gear| {
            let mut keep = true;
            ui.push_id(("motion gear", id), |ui| {
                ui.collapsing(format!("Gear driving {id}"), |ui| {
                    choose(ui, "Driver", &mut gear.driver, &hinges);
                    ui.horizontal(|ui| {
                        ui.label("Driven angle / driver angle");
                        ui.add(egui::DragValue::new(&mut gear.ratio).speed(0.05));
                    });
                    keep = !ui.button("Remove gear relation").clicked();
                });
            });
            keep
        });
        draft.linkages.retain(|id, link| {
            let mut keep = true;
            ui.push_id(("motion rod", id), |ui| {
                ui.collapsing(format!("Linkage {id}"), |ui| {
                    choose(ui, "Driver", &mut link.driver, &hinges);
                    choose(ui, "Follower", &mut link.follower, &hinges);
                    coordinates(ui, "Driver pin (mm)", &mut link.driver_pin);
                    coordinates(ui, "Follower pin (mm)", &mut link.follower_pin);
                    let mut sockets = link.socket_axes.is_some();
                    if ui.checkbox(&mut sockets, "Set ball stud axes").changed() {
                        link.socket_axes = sockets.then_some([[0.0, 0.0, 1.0]; 2]);
                    }
                    if let Some([a, b]) = &mut link.socket_axes {
                        coordinates(ui, "Driver stud axis", a);
                        coordinates(ui, "Follower stud axis", b);
                    }
                    part_choices(ui, data, &mut link.rod_parts);
                    keep = !ui.button("Remove linkage").clicked();
                });
            });
            keep
        });
        if hinges.len() >= 2 {
            ui.collapsing("Add relation", |ui| {
                if !hinges.contains(&self.driver) {
                    self.driver.clone_from(&hinges[0]);
                }
                choose(ui, "Driver", &mut self.driver, &hinges);
                let available: Vec<_> = hinges
                    .iter()
                    .filter(|id| {
                        **id != self.driver
                            && !draft.gears.contains_key(*id)
                            && !draft.linkages.values().any(|link| link.follower == **id)
                    })
                    .cloned()
                    .collect();
                if available.is_empty() {
                    ui.label("All other hinges have a driver");
                    return;
                }
                if !available.contains(&self.follower) {
                    self.follower.clone_from(&available[0]);
                }
                choose(ui, "Follower", &mut self.follower, &available);
                if ui.button("Add gear relation").clicked() {
                    draft.gears.insert(
                        self.follower.clone(),
                        Gear {
                            driver: self.driver.clone(),
                            ratio: -1.0,
                        },
                    );
                }
                ui.label("Linkage name");
                ui.text_edit_singleline(&mut self.linkage_name);
                let name = self.linkage_name.trim();
                if ui
                    .add_enabled(
                        !name.is_empty() && !draft.linkages.contains_key(name),
                        egui::Button::new("Add four-bar linkage"),
                    )
                    .clicked()
                {
                    draft.linkages.insert(
                        name.to_string(),
                        FourBar {
                            driver: self.driver.clone(),
                            follower: self.follower.clone(),
                            driver_pin: draft.joints[&self.driver].origin,
                            follower_pin: draft.joints[&self.follower].origin,
                            socket_axes: None,
                            rod_parts: Vec::new(),
                        },
                    );
                    self.linkage_name.clear();
                }
            });
        }
        if let Some(error) = &self.error {
            ui.label(error);
        }
        if ui.button("Save joints and relations").clicked() {
            match draft.solve(&Default::default()) {
                Ok(_) => {
                    self.error = None;
                    return self.draft.take();
                }
                Err(error) => self.error = Some(error),
            }
        }
        if ui.button("Cancel motion edits").clicked() {
            *self = Self::default();
        }
        None
    }
}

fn choose(ui: &mut egui::Ui, label: &str, selected: &mut String, options: &[String]) {
    egui::ComboBox::from_label(label)
        .selected_text(if selected.is_empty() {
            "Assembly"
        } else {
            selected.as_str()
        })
        .show_ui(ui, |ui| {
            for id in options {
                ui.selectable_value(
                    selected,
                    id.clone(),
                    if id.is_empty() { "Assembly" } else { id },
                );
            }
        });
}

fn coordinates(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3]) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        for (axis, v) in ["X ", "Y ", "Z "].into_iter().zip(value) {
            ui.add(egui::DragValue::new(v).speed(0.1).prefix(axis));
        }
    });
}

fn part_choices(ui: &mut egui::Ui, data: &ProjectData, selected: &mut Vec<String>) {
    egui::CollapsingHeader::new(format!("Moving parts ({})", selected.len()))
        .id_salt("members")
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for (id, part) in &data.parts {
                        let mut active = selected.contains(id);
                        if ui
                            .checkbox(&mut active, format!("{} ({id})", part.name))
                            .changed()
                        {
                            if active {
                                selected.push(id.clone());
                            } else {
                                selected.retain(|old| old != id);
                            }
                        }
                    }
                });
        });
}
