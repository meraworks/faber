use super::*;
use faber_wiring::animate::{self, Animation, AnimationOptions};
use faber_wiring::studio::Style;
use std::path::Path;
use tempfile::TempPath;

#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
    #[default]
    Turntable,
    Explode,
    Motion,
    Views,
}

pub(super) struct Export {
    open: bool,
    kind: Kind,
    name: String,
    from: f64,
    to: f64,
    turns: f64,
    joint: String,
    names: Vec<String>,
    add_view: String,
    frames: u32,
    fps: u32,
    width: u32,
    height: u32,
    job: Option<Job>,
    message: Option<String>,
}
impl Default for Export {
    fn default() -> Self {
        Self {
            open: false,
            kind: Default::default(),
            name: String::new(),
            from: 0.,
            to: 1.,
            turns: 1.,
            joint: String::new(),
            names: vec![],
            add_view: String::new(),
            frames: 24,
            fps: 24,
            width: 640,
            height: 480,
            job: None,
            message: None,
        }
    }
}
struct Job {
    sink: PathBuf,
    revision: u64,
    stamp: Option<(SystemTime, u64)>,
    receiver: mpsc::Receiver<Result<TempPath, String>>,
}
impl Export {
    fn animation(&self) -> Animation {
        match self.kind {
            Kind::Turntable => Animation::Turntable { turns: self.turns },
            Kind::Explode => Animation::Explode {
                from: self.from,
                to: self.to,
            },
            Kind::Motion => Animation::Motion {
                joint: self.joint.clone(),
                from: self.from,
                to: self.to,
            },
            Kind::Views => Animation::Views {
                names: self.names.clone(),
            },
        }
    }
}
impl Shell {
    pub(in crate::app) fn poll_animation(&mut self) {
        let state = &mut self.views.export;
        let Some(job) = &state.job else { return };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The animation worker stopped.".into()),
        };
        let job = state.job.take().unwrap();
        state.message = Some(
            match result.and_then(|file| {
                if self.pending_project.is_some()
                    || self.data.revision != job.revision
                    || faber_wiring::project_fingerprint(&self.root).ok() != job.stamp
                {
                    return Err(
                        "The project changed during export. Write the animation again.".into(),
                    );
                }
                file.persist(&job.sink).map_err(|e| e.to_string())?;
                Ok(())
            }) {
                Ok(()) => format!("Saved {}", job.sink.display()),
                Err(e) => e,
            },
        );
    }

    fn begin_animation(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let (view, section) = self.snapshot_view();
        let state = &mut self.views.export;
        let sink = Path::new(state.name.trim());
        if !sink
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("png") || s.eq_ignore_ascii_case("apng"))
        {
            return Err("Use a .png or .apng file.".into());
        }
        let sink = self.root.join(sink);
        let mut id = "current_view".to_owned();
        while self.data.views.contains_key(&id) {
            id.push('_');
        }
        let opts = AnimationOptions {
            frames: state.frames,
            fps: state.fps,
            width: state.width,
            height: state.height,
            view: Some(id.clone()),
            style: if self.views.studio {
                Style::Studio
            } else {
                Style::Technical
            },
            frames_dir: None,
        };
        let studio = faber_wiring::studio::Options {
            style: opts.style,
            ground: self.views.style.ground,
            background: self.views.style.background,
            exposure: self.views.style.exposure,
            ..Default::default()
        };
        let request = state.animation();
        let root = self.root.clone();
        let revision = self.data.revision;
        let stamp = faber_wiring::project_fingerprint(&root)?;
        if Some(stamp) != self.modified && !self.saving {
            return Err("The project changed. Wait for it to reload.".into());
        }
        let dir = sink
            .parent()
            .ok_or("Choose the output directory.")?
            .to_path_buf();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        state.job = Some(Job {
            sink,
            revision,
            stamp: Some(stamp),
            receiver: rx,
        });
        state.message = None;
        #[cfg(test)]
        let gate = self.views.export_gate.take();
        std::thread::spawn(move || {
            let result = (|| {
                let mut project = faber_wiring::read_snapshot(&root)?;
                if project.data.revision != revision
                    || faber_wiring::project_fingerprint(&root)? != stamp
                {
                    return Err(
                        "The project changed before export. Write the animation again.".into(),
                    );
                }
                if let Some((id, section)) = section {
                    project.data.sections.insert(id, section);
                }
                project.data.views.insert(id, view);
                let file = tempfile::Builder::new()
                    .prefix(".anim-")
                    .suffix(".apng")
                    .tempfile_in(dir)
                    .map_err(|e| e.to_string())?
                    .into_temp_path();
                animate::with_studio(&project, &request, &opts, &file, &studio)?;
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate.recv()
                        .map_err(|_| "The test export stopped.".to_string())?;
                }
                Ok(file)
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        Ok(())
    }
    pub(super) fn animation_panel(&mut self, ui: &mut egui::Ui) {
        let state = &mut self.views.export;
        if !state.open {
            if ui.button("Export animation").clicked() {
                state.open = true;
            }
            return;
        }
        ui.heading("Export animation");
        let running = state.job.is_some();
        let mut write = false;
        ui.add_enabled_ui(!running, |ui| {
            let modes = [
                (Kind::Turntable, "Turntable"),
                (Kind::Explode, "Exploded assembly"),
                (Kind::Motion, "Joint motion"),
                (Kind::Views, "Named views"),
            ];
            let mode = modes.iter().find(|(k, _)| k == &state.kind).unwrap().1;
            let old = state.kind;
            egui::ComboBox::from_label("Animation kind")
                .selected_text(mode)
                .show_ui(ui, |ui| {
                    for (k, t) in modes {
                        ui.selectable_value(&mut state.kind, k, t);
                    }
                });
            if old != state.kind {
                state.from = 0.;
                state.to = 1.;
                state.message = None;
            }
            match state.kind {
                Kind::Turntable => {
                    ui.horizontal(|ui| {
                        ui.label("Turns");
                        ui.add(egui::DragValue::new(&mut state.turns).speed(0.05));
                    });
                }
                Kind::Motion | Kind::Explode => {
                    if state.kind == Kind::Motion {
                        let joints = self
                            .data
                            .motion
                            .joints
                            .iter()
                            .filter(|(_, j)| j.kind.count() == 1);
                        let was = state.joint.clone();
                        if state.joint.is_empty() {
                            state.joint = joints
                                .clone()
                                .next()
                                .map_or_else(String::new, |(s, _)| s.clone());
                        }
                        egui::ComboBox::from_label("Joint")
                            .selected_text(&state.joint)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for (id, _) in joints {
                                    ui.selectable_value(&mut state.joint, id.clone(), id);
                                }
                            });
                        if was != state.joint
                            && let Some([lo, hi]) = self
                                .data
                                .motion
                                .joints
                                .get(&state.joint)
                                .and_then(|j| j.limits)
                        {
                            state.from = lo;
                            state.to = hi;
                        }
                    }
                    for (label, val) in [("From", &mut state.from), ("To", &mut state.to)] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add(egui::DragValue::new(val).speed(0.05));
                        });
                    }
                }
                Kind::Views => {
                    if state.add_view.is_empty()
                        && let Some((id, _)) = self.data.views.first_key_value()
                    {
                        state.add_view = id.clone();
                    }
                    egui::ComboBox::from_label("Next view")
                        .selected_text(&state.add_view)
                        .truncate()
                        .show_ui(ui, |ui| {
                            for id in self.data.views.keys() {
                                ui.selectable_value(&mut state.add_view, id.clone(), id);
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!state.add_view.is_empty(), egui::Button::new("Add view"))
                            .clicked()
                        {
                            state.names.push(state.add_view.clone());
                        }
                        if ui
                            .add_enabled(!state.names.is_empty(), egui::Button::new("Remove last"))
                            .clicked()
                        {
                            state.names.pop();
                        }
                    });
                    ui.label(state.names.join(" → "));
                }
            }
            egui::Grid::new("animation_settings").show(ui, |ui| {
                for (label, v, r) in [
                    ("Frames", &mut state.frames, 2..=600),
                    ("Frames per second", &mut state.fps, 1..=240),
                    ("Width", &mut state.width, 64..=4096),
                    ("Height", &mut state.height, 64..=4096),
                ] {
                    let l = ui.label(label);
                    ui.add(egui::DragValue::new(v).range(r)).labelled_by(l.id);
                    ui.end_row();
                }
            });
            let label = ui.label("Animation file");
            ui.text_edit_singleline(&mut state.name)
                .labelled_by(label.id);
            write = ui
                .add_enabled(
                    !state.name.trim().is_empty()
                        && self.pending.is_none()
                        && match state.kind {
                            Kind::Motion => !state.joint.is_empty(),
                            Kind::Views => state.names.len() >= 2,
                            _ => true,
                        },
                    egui::Button::new("Write animation"),
                )
                .clicked();
        });
        if running {
            ui.weak("Writing animation…");
        }
        if let Some(message) = &state.message {
            ui.label(message);
        }
        if write && let Err(e) = self.begin_animation(ui.ctx()) {
            self.views.export.message = Some(e);
        }
    }
}
