//! Inspect and edit imported mesh assets without blocking the modelling view.
use super::*;
use faber_wiring::mesh_edit::{self, Keep, MeshEdit, MeshReport, RepairOptions, Target};
use std::sync::mpsc::TryRecvError;

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Report,
    Repair,
    Reduce,
    Cut,
    Solid,
}
const ACTIONS: [(Action, &str); 5] = [
    (Action::Report, "Mesh report"),
    (Action::Repair, "Repair"),
    (Action::Reduce, "Reduce"),
    (Action::Cut, "Plane cut"),
    (Action::Solid, "Convert to solid"),
];
enum Job {
    Read(MeshReport),
    Edited {
        part: String,
        message: String,
        report: Result<Option<MeshReport>, String>,
        revision: u64,
    },
}

pub(super) struct Panel {
    root: PathBuf,
    part: String,
    revision: u64,
    purchased: bool,
    action: Action,
    worker: Option<mpsc::Receiver<Result<Job, String>>>,
    report: Option<MeshReport>,
    message: Option<String>,
    error: Option<String>,
    repair: RepairOptions,
    dust: bool,
    dust_limit: f64,
    target_mode: usize,
    count: usize,
    ratio: f64,
    max_error: f64,
    origin: [f64; 3],
    normal: [f64; 3],
    side: usize,
    cap: bool,
    copy: String,
    merge: bool,
    converted: bool,
}

impl Panel {
    fn new(shell: &Shell, id: String, action: Action, purchased: bool) -> Self {
        let body = shell
            .bodies
            .iter()
            .find(|b| b.id == id)
            .expect("Selected mesh body");
        Self {
            root: shell.root.clone(),
            part: id,
            revision: shell.data.revision,
            purchased,
            action,
            worker: None,
            report: None,
            message: None,
            error: None,
            repair: RepairOptions::default(),
            dust: false,
            dust_limit: 1.,
            target_mode: 0,
            count: (body.mesh.triangles.len() / 2).max(4),
            ratio: 0.5,
            max_error: 0.1,
            origin: std::array::from_fn(|i| (body.bounds[0][i] + body.bounds[1][i]) / 2.),
            normal: [0., 0., 1.],
            side: 1,
            cap: true,
            copy: String::new(),
            merge: true,
            converted: false,
        }
    }
    fn operation(&self) -> MeshEdit {
        let copy_as = (!self.copy.trim().is_empty()).then(|| self.copy.trim().to_owned());
        match self.action {
            Action::Repair => MeshEdit::Repair(RepairOptions {
                drop_shells_below_mm3: self.dust.then_some(self.dust_limit),
                ..self.repair.clone()
            }),
            Action::Reduce => MeshEdit::Reduce(match self.target_mode {
                0 => Target::Triangles(self.count),
                1 => Target::Ratio(self.ratio),
                _ => Target::MaxError(self.max_error),
            }),
            Action::Cut => MeshEdit::Cut {
                origin: self.origin,
                normal: self.normal,
                keep: match self.side {
                    0 => Keep::Above,
                    1 => Keep::Below,
                    _ => Keep::Both,
                },
                cap: self.cap,
                copy_as,
            },
            Action::Solid => MeshEdit::ToSolid {
                merge_planar: self.merge,
                copy_as,
            },
            Action::Report => unreachable!("The report has no edit"),
        }
    }
    fn actions(&mut self, ui: &mut egui::Ui, enabled: bool) -> bool {
        if self.purchased {
            ui.small("This is a purchased part. Its geometry is read-only.");
        }
        if self.purchased || self.converted {
            return false;
        }
        ui.separator();
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (action, label) in ACTIONS.into_iter().skip(1) {
                    ui.selectable_value(&mut self.action, action, label);
                }
            });
            self.fields(ui);
            let label = match self.action {
                Action::Report => return false,
                Action::Repair => "Apply repair",
                Action::Reduce => "Apply reduction",
                Action::Cut => "Apply plane cut",
                Action::Solid => "Create solid",
            };
            ui.button(label).clicked()
        })
        .inner
    }

    fn fields(&mut self, ui: &mut egui::Ui) {
        match self.action {
            Action::Report => {}
            Action::Repair => {
                number(
                    ui,
                    "Weld tolerance",
                    &mut self.repair.weld_mm,
                    0.000001,
                    " mm",
                );
                ui.checkbox(&mut self.repair.fix_winding, "Correct triangle winding");
                ui.checkbox(&mut self.repair.fill_holes, "Fill holes");
                if self.repair.fill_holes {
                    number(
                        ui,
                        "Maximum edges per hole",
                        &mut self.repair.max_hole_edges,
                        1.,
                        "",
                    );
                }
                ui.checkbox(&mut self.dust, "Remove small closed shells");
                if self.dust {
                    number(ui, "Volume threshold", &mut self.dust_limit, 0.1, " mm³");
                }
            }
            Action::Reduce => {
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in ["Triangles", "Ratio", "Error"].into_iter().enumerate() {
                        ui.selectable_value(&mut self.target_mode, i, s);
                    }
                });
                match self.target_mode {
                    0 => number(ui, "Target triangles", &mut self.count, 1., ""),
                    1 => number(ui, "Fraction to keep", &mut self.ratio, 0.01, ""),
                    _ => number(ui, "Maximum error", &mut self.max_error, 0.01, " mm"),
                }
            }
            Action::Cut => {
                ui.label("World coordinates");
                vector(ui, "Plane origin", &mut self.origin, " mm");
                vector(ui, "Plane normal", &mut self.normal, "");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Keep");
                    for (i, s) in ["Above", "Below", "Both"].into_iter().enumerate() {
                        ui.selectable_value(&mut self.side, i, s);
                    }
                });
                ui.checkbox(&mut self.cap, "Cap cut faces");
                copy_field(ui, &mut self.copy, self.side == 2);
            }
            Action::Solid => {
                ui.checkbox(&mut self.merge, "Merge coplanar faces");
                copy_field(ui, &mut self.copy, false);
            }
        }
    }
}

fn number(
    ui: &mut egui::Ui,
    label: &str,
    n: &mut impl egui::emath::Numeric,
    speed: f64,
    suffix: &str,
) {
    ui.horizontal(|ui| {
        let l = ui.label(label);
        ui.add(egui::DragValue::new(n).speed(speed).suffix(suffix))
            .labelled_by(l.id);
    });
}
fn vector(ui: &mut egui::Ui, label: &str, v: &mut [f64; 3], suffix: &str) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        for (axis, n) in ["X", "Y", "Z"].into_iter().zip(v.iter_mut()) {
            ui.add(
                egui::DragValue::new(n)
                    .speed(0.1)
                    .prefix(format!("{axis} "))
                    .suffix(suffix),
            );
        }
    });
}
fn copy_field(ui: &mut egui::Ui, id: &mut String, both: bool) {
    let label = ui.label(if both {
        "New part id for the upper side"
    } else {
        "New part id (optional)"
    });
    ui.text_edit_singleline(id).labelled_by(label.id);
    if both {
        ui.small("The lower side keeps the original id.");
    } else {
        ui.small("Enter a new id to keep the original.");
    }
}

impl Shell {
    pub(super) fn mesh_menu(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(part) = self.data.parts.get(&id) else {
            return;
        };
        let faber_model::Geometry::Imported { purchased, .. } = &part.geometry else {
            return;
        };
        if !self.extra_selected.is_empty()
            || !self
                .bodies
                .iter()
                .any(|body| body.id == id && body.shape.is_none())
        {
            return;
        }
        let purchased = purchased.is_some();
        let idle = self.pending.is_none()
            && self
                .mesh_tool
                .as_ref()
                .is_none_or(|panel| panel.worker.is_none());
        for (action, label) in ACTIONS {
            if ui
                .add_enabled(
                    idle && (action == Action::Report || !purchased),
                    egui::Button::new(label),
                )
                .on_disabled_hover_text(if purchased {
                    "This is a purchased part."
                } else {
                    "Wait for the current operation."
                })
                .clicked()
            {
                self.mesh_tool = Some(Panel::new(self, id.clone(), action, purchased));
                self.start_mesh_job(ui.ctx(), false);
                ui.close();
                break;
            }
        }
        ui.separator();
    }
    fn start_mesh_job(&mut self, ctx: &egui::Context, edit: bool) {
        let Some(panel) = self.mesh_tool.as_mut() else {
            return;
        };
        if panel.worker.is_some() || (edit && self.pending.is_some()) {
            return;
        }
        let (root, part, revision) = (panel.root.clone(), panel.part.clone(), panel.revision);
        let operation = edit.then(|| panel.operation());
        let solid = panel.action == Action::Solid;
        let copy_target = if matches!(panel.action, Action::Solid | Action::Cut)
            && !panel.copy.trim().is_empty()
        {
            Some(panel.copy.trim().to_owned())
        } else {
            None
        };
        let (tx, rx) = mpsc::channel();
        panel.worker = Some(rx);
        panel.error = None;
        let snapshot = if edit {
            let (sender, receiver) = mpsc::channel();
            self.pending = Some(receiver);
            self.saving = true;
            Some(sender)
        } else {
            None
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut project = faber_wiring::open(&root, false, None)?;
                if project.data.revision != revision {
                    return Err(
                        "The project changed. Reopen Mesh tools for the current revision."
                            .to_owned(),
                    );
                }
                let Some(op) = operation else {
                    return mesh_edit::report(&project, &part).map(Job::Read);
                };
                let result = mesh_edit::apply(&mut project, &part, revision, op)?;
                let target = copy_target.unwrap_or(part);
                let report = if solid {
                    None
                } else {
                    Some(mesh_edit::report(&project, &target))
                };
                if let Some(sender) = &snapshot {
                    let _ = sender.send(Ok((project.data, project.bodies)));
                }
                let message = operation_summary(&result.log, &result.parts, result.changed);
                Ok(Job::Edited {
                    part: target,
                    message,
                    report: report.transpose(),
                    revision: result.revision,
                })
            })();
            if let Err(e) = &result
                && let Some(sender) = snapshot
            {
                let _ = sender.send(Err(e.clone()));
            }
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub(super) fn poll_mesh_panel(&mut self) {
        let Some(panel) = self.mesh_tool.as_mut() else {
            return;
        };
        if panel.root != self.root {
            self.mesh_tool = None;
            return;
        }
        let Some(worker) = &panel.worker else { return };
        let result = match worker.try_recv() {
            Ok(v) => v,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The mesh worker stopped.".into()),
        };
        panel.worker = None;
        match result {
            Ok(Job::Read(r)) if self.data.revision == panel.revision => panel.report = Some(r),
            Ok(Job::Read(_)) => {
                panel.error =
                    Some("The project changed. Reopen Mesh tools for the current revision.".into())
            }
            Ok(Job::Edited {
                part,
                message,
                report,
                revision,
            }) => {
                panel.converted = matches!(report, Ok(None));
                panel.part = part.clone();
                panel.copy.clear();
                self.selected = Some(part);
                self.extra_selected.clear();
                panel.message = Some(message);
                match report {
                    Ok(report) => panel.report = report,
                    Err(message) => {
                        panel.report = None;
                        panel.error = Some(message);
                    }
                }
                panel.revision = revision;
            }
            Err(e) => panel.error = Some(e),
        }
    }
    pub(super) fn mesh_panel(&mut self, ui: &mut egui::Ui) {
        let panel = self.mesh_tool.as_mut().unwrap();
        ui.heading("Mesh tools");
        ui.label(&panel.part);
        let working = panel.worker.is_some();
        if ui
            .add_enabled(!working, egui::Button::new("Close mesh tools"))
            .clicked()
        {
            self.mesh_tool = None;
            return;
        }
        let apply = panel.actions(
            ui,
            !working && self.pending.is_none() && panel.revision == self.data.revision,
        );
        if let Some(r) = &panel.report {
            ui.separator();
            ui.strong(if r.watertight {
                "Watertight"
            } else {
                "Open or inconsistent mesh"
            });
            ui.label(format!(
                "{} vertices · {} triangles · {} shells",
                r.vertices, r.triangles, r.shells
            ));
            ui.label(format!("Surface area {:.3} mm²", r.area_mm2));
            if let Some(v) = r.volume_mm3 {
                ui.label(format!("Signed volume {v:.3} mm³"));
            }
            for issue in &r.problems {
                ui.label(issue);
            }
        }
        if let Some(message) = &panel.message {
            ui.label(message);
        }
        if let Some(error) = &panel.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if working {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working on mesh…");
            });
        }
        if !working && panel.revision != self.data.revision && panel.error.is_none() {
            ui.label("The project changed. Reopen Mesh tools for the current revision.");
        }
        if apply {
            self.start_mesh_job(ui.ctx(), true);
        }
    }
}

fn operation_summary(log: &serde_json::Value, parts: &[String], changed: bool) -> String {
    if !changed {
        return "The mesh is unchanged.".to_owned();
    }
    if let Some(after) = log.get("after")
        && let Some(before) = log.get("before")
    {
        return format!("Reduced from {before} to {after} triangles.");
    }
    if let Some(holes) = log.get("filled_holes") {
        return format!("Filled {holes} holes. Updated {}.", parts.join(", "));
    }
    format!("Saved {}.", parts.join(", "))
}
