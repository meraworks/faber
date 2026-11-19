//! Drag the selected plane along its normal.
use super::*;
use faber_wiring::press_pull::{self as edit, PressFace};
use glam::DVec3;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// Finish the active kernel operation before the next preview or save starts.
static BUILD: Mutex<()> = Mutex::new(());

enum Reply {
    Ready(Result<Box<PressFace>, String>),
    Preview(f64, Result<Arc<Vec<Body>>, String>),
}

pub(super) struct Tool {
    root: PathBuf,
    original: ProjectData,
    pub(super) face: Option<PressFace>,
    pub(super) distance: f64,
    exact: bool,
    pub(super) preview: Option<Arc<Vec<Body>>>,
    latest: Arc<AtomicU64>,
    tx: mpsc::Sender<f64>,
    rx: mpsc::Receiver<Reply>,
    drag: Option<(egui::Pos2, f64, Vec2, f64)>,
    error: Option<String>,
    pub(super) submitting: bool,
}

impl Shell {
    pub(super) fn start_press(&mut self, ctx: &egui::Context) {
        self.start_face_drag(ctx, false);
    }

    pub(super) fn start_face_drag(&mut self, ctx: &egui::Context, exact: bool) {
        let Some(Pick::Face { part, face }) = self.picks.last().cloned() else {
            return;
        };
        let indices = self
            .picks
            .iter()
            .filter_map(|p| match p {
                Pick::Face { part: id, face } if id == &part => Some(*face),
                _ => None,
            })
            .collect::<Vec<_>>();
        if exact && indices.len() != self.picks.len() {
            self.error = Some("Pick faces on one part.".into());
            return;
        }
        let point = self.pick_point.map(|p| {
            self.display_pose(&part)
                .as_dmat4()
                .inverse()
                .transform_point3(DVec3::from(p))
                .into()
        });
        let root = self.root.clone();
        let original = self.data.clone();
        let (tx, requests) = mpsc::channel::<f64>();
        let (answers, rx) = mpsc::channel();
        let current = Arc::new(AtomicU64::new(0));
        let latest = Arc::downgrade(&current);
        self.press = Some(Tool {
            root: root.clone(),
            original: original.clone(),
            face: None,
            distance: 0.,
            exact,
            preview: None,
            latest: current,
            tx,
            rx,
            drag: None,
            error: None,
            submitting: false,
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let setup_guard = BUILD.lock().unwrap_or_else(|e| e.into_inner());
            if latest.strong_count() == 0 {
                return;
            }
            let setup = faber_wiring::preview_project(&root, original).and_then(|project| {
                let target = if exact {
                    PressFace::move_faces(&project, &part, &indices, point)?
                } else {
                    PressFace::new(&project, &part, face)?
                };
                Ok((project, target))
            });
            drop(setup_guard);
            let (project, target) = match setup {
                Ok(value) => value,
                Err(reason) => {
                    let _ = answers.send(Reply::Ready(Err(reason)));
                    ctx.request_repaint();
                    return;
                }
            };
            if answers
                .send(Reply::Ready(Ok(Box::new(target.clone()))))
                .is_err()
            {
                return;
            }
            ctx.request_repaint();
            while let Ok(mut distance) = requests.recv() {
                loop {
                    match requests.try_recv() {
                        Ok(new) => distance = new,
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                if distance == 0. {
                    continue;
                }
                let guard = BUILD.lock().unwrap_or_else(|e| e.into_inner());
                if !latest
                    .upgrade()
                    .is_some_and(|slot| slot.load(Ordering::Relaxed) == distance.to_bits())
                {
                    continue;
                }
                let body = edit::preview_scene(&project, &target, distance).map(Arc::new);
                drop(guard);
                if answers.send(Reply::Preview(distance, body)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn close_press(&mut self) {
        if self.press.take().is_some_and(|tool| tool.preview.is_some()) {
            self.list = None;
            self.dirty = true;
        }
    }

    pub(super) fn poll_press(&mut self) {
        let Some(tool) = &mut self.press else { return };
        if tool.original != self.data || tool.root != self.root {
            self.close_press();
            return;
        }
        while let Ok(reply) = tool.rx.try_recv() {
            match reply {
                Reply::Ready(Ok(face)) => tool.face = Some(*face),
                Reply::Ready(Err(reason)) => {
                    self.error = Some(reason);
                    self.close_press();
                    return;
                }
                Reply::Preview(d, result) if d == tool.distance => match result {
                    Ok(bodies) => {
                        tool.preview = Some(bodies);
                        tool.error = None;
                        self.list = None;
                        self.dirty = true;
                    }
                    Err(reason) => {
                        tool.error = Some(reason);
                        if tool.preview.take().is_some() {
                            self.list = None;
                            self.dirty = true;
                        }
                    }
                },
                _ => (),
            }
        }
    }

    fn press_distance(&mut self, distance: f64) {
        let Some(tool) = &mut self.press else { return };
        if tool.submitting || distance == tool.distance || !distance.is_finite() {
            return;
        }
        tool.latest.store(distance.to_bits(), Ordering::Relaxed);
        tool.distance = distance;
        tool.error = None;
        if distance == 0. && tool.preview.take().is_some() {
            self.list = None;
            self.dirty = true;
        }
        let _ = tool.tx.send(distance);
    }

    pub(super) fn apply_press(&mut self, ctx: &egui::Context) {
        let Some(tool) = &self.press else { return };
        if self.pending.is_some() || tool.distance == 0. || tool.error.is_some() {
            return;
        }
        let Some(face) = tool.face.clone() else {
            return;
        };
        tool.latest.store(0, Ordering::Relaxed);
        let (root, expected, distance) = (tool.root.clone(), tool.original.clone(), tool.distance);
        let strict = self.strict_edits;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.press.as_mut().unwrap().submitting = true;
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                let _guard = BUILD.lock().unwrap_or_else(|e| e.into_inner());
                let mut project = faber_wiring::open(&root, false, None)?;
                if project.data != expected {
                    return Err("Project changed. Pick the face again.".to_owned());
                }
                edit::apply(&mut project, &face, distance, strict)?;
                Ok((project.data, project.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn press_arrow(&self, rect: egui::Rect) -> Option<(egui::Pos2, egui::Pos2, f64)> {
        let tool = self.press.as_ref()?;
        let face = tool.face.as_ref()?;
        let pose = self.display_pose(&face.part).as_dmat4();
        let normal = pose
            .transform_vector3(DVec3::from(face.normal))
            .normalize_or_zero();
        let point = pose.transform_point3(DVec3::from(face.centre)) + normal * tool.distance;
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let start = move_tool::screen_point(rect, matrix, point)?;
        let step = move_tool::screen_point(rect, matrix, point + normal)? - start;
        let nominal = (rect.height() / self.camera.distance.max(0.01)).max(0.01);
        let (dir, scale) = if step.length() > nominal * 0.1 {
            (step.normalized(), step.length())
        } else {
            (Vec2::UP, nominal)
        };
        Some((start, start + dir * 60., 1. / f64::from(scale)))
    }

    pub(super) fn press_interact(&mut self, ui: &egui::Ui, rect: egui::Rect) -> bool {
        let Some((start, tip, mm_per_px)) = self.press_arrow(rect) else {
            return false;
        };
        let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else {
            return false;
        };
        let axis = (tip - start).normalized();
        let along = (pos - start).dot(axis).clamp(0., 60.);
        let hit = (start + axis * along).distance(pos) <= 12.
            && rect.contains(pos)
            && ui.rect_contains_pointer(rect);
        if hit && ui.input(|i| i.pointer.primary_pressed()) {
            let tool = self.press.as_mut().unwrap();
            tool.drag = Some((pos, tool.distance, axis, mm_per_px));
        }
        let Some((origin, value, dir, scale)) = self.press.as_ref().and_then(|t| t.drag) else {
            return false;
        };
        self.press_distance(value + f64::from((pos - origin).dot(dir)) * scale);
        if ui.input(|i| i.pointer.primary_released()) {
            self.press.as_mut().unwrap().drag = None;
            self.apply_press(ui.ctx());
        }
        true
    }

    pub(super) fn press_paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        if let Some((start, tip, _)) = self.press_arrow(rect) {
            let p = ui.painter().with_clip_rect(rect);
            let colour = Color32::from_rgb(0, 139, 199);
            p.circle_filled(start, 4., colour);
            p.arrow(start, tip - start, egui::Stroke::new(3.5, colour));
        }
    }

    pub(super) fn press_panel(&mut self, ctx: &egui::Context) {
        let Some(tool) = &self.press else { return };
        let mut value = tool.distance;
        let ready = tool.face.is_some() && !tool.submitting;
        let submitting = tool.submitting;
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if tool.exact {
            "Move face"
        } else {
            "Press pull"
        })
        .id(egui::Id::unique("press-pull-distance"))
        .anchor(egui::Align2::RIGHT_TOP, [-310., 90.])
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 28.).clamp(125., 270.));
            ui.horizontal(|ui| {
                let label = ui.label("Distance");
                ui.add_enabled(
                    ready,
                    egui::DragValue::new(&mut value).speed(0.1).suffix(" mm"),
                )
                .labelled_by(label.id);
            });
            ui.horizontal(|ui| {
                apply = ui
                    .add_enabled(
                        ready && value != 0. && tool.error.is_none(),
                        egui::Button::new("Apply"),
                    )
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
            if let Some(reason) = &tool.error {
                ui.colored_label(ui.visuals().error_fg_color, reason);
            }
        });
        self.press_distance(value);
        if submitting {
            return;
        }
        if cancel || !open {
            self.close_press()
        } else if apply {
            self.apply_press(ctx)
        }
    }
}
