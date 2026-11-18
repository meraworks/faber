//! Read-only drawing sheets in paper coordinates.
use super::*;
use faber_model::drawing::SheetSize;
use faber_ports::vector::{Anchor, Path2, VectorDoc};
use faber_wiring::drawing::{self, Rendered};
use std::path::Path;
use std::sync::{Arc, mpsc::TryRecvError};

type Stamp = (std::time::SystemTime, u64);
type Page = (Rendered, Stamp, SheetSize);
const CHANGED: &str = "The project changed. Read the current drawing revision.";

#[derive(Clone, Copy, PartialEq, Eq)]
struct Version {
    revision: u64,
    generation: u64,
}

#[derive(Clone, PartialEq)]
struct Key {
    root: PathBuf,
    id: String,
    version: Version,
}
type Job<T> = (Key, mpsc::Receiver<Result<T, String>>);
#[derive(Default)]
pub(super) struct Sheets {
    generation: u64,
    pub(super) view: Option<SheetView>,
    render: Option<Job<Page>>,
    export: Option<Job<String>>,
}
pub(super) struct SheetView {
    root: PathBuf,
    id: String,
    requested: Option<Version>,
    shown: Option<Version>,
    pub(super) doc: Option<Arc<Rendered>>,
    source: Option<(Stamp, SheetSize)>,
    error: Option<String>,
    path: String,
    output: Option<String>,
    zoom: f32,
    pan: egui::Vec2,
}
impl Sheets {
    pub(super) fn project_loaded(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn version(&self, revision: u64) -> Version {
        Version {
            revision,
            generation: self.generation,
        }
    }

    pub(super) fn file_chosen(&mut self, root: &std::path::Path, id: &str, path: PathBuf) {
        if let Some(v) = &mut self.view
            && v.root == root
            && v.id == id
        {
            v.path = path.display().to_string();
        }
    }
}
fn ready<T>(job: &mut Option<Job<T>>) -> Option<(Key, Result<T, String>)> {
    let (_, rx) = job.as_ref()?;
    let result = match rx.try_recv() {
        Ok(r) => r,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => Err("The drawing worker stopped.".into()),
    };
    Some((job.take()?.0, result))
}
fn check_stamp(root: &Path, stamp: Stamp) -> Result<(), String> {
    if faber_wiring::project_fingerprint(root)? != stamp {
        return Err(CHANGED.into());
    }
    Ok(())
}

fn read_page(key: &Key) -> Result<Page, String> {
    let stamp = faber_wiring::project_fingerprint(&key.root)?;
    let project = drawing::open_snapshot(&key.root)?;
    if project.data.revision != key.version.revision {
        return Err(CHANGED.into());
    }
    let size = project
        .data
        .drawings
        .get(&key.id)
        .ok_or(CHANGED)?
        .sheet
        .size;
    let page = drawing::render(&project, &key.id)?;
    check_stamp(&key.root, stamp)?;
    Ok((page, stamp, size))
}

/// Publish the displayed revision only after its file is complete.
fn export_page(root: &Path, stamp: Stamp, page: &Rendered, path: &Path) -> Result<String, String> {
    check_stamp(root, stamp)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or("Use .svg, .pdf, .dxf or .png for a drawing.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?
        .into_temp_path();
    drawing::write_rendered(page, &temp, None)?;
    check_stamp(root, stamp)?;
    temp.persist(path)
        .map_err(|e| format!("Cannot export {}: {e}", path.display()))?;
    Ok(format!("Exported {}.", path.display()))
}
impl Shell {
    pub(super) fn drawing_list(&mut self, ui: &mut egui::Ui) {
        if self.data.drawings.is_empty() {
            return;
        }
        ui.label(RichText::new("Drawings").strong());
        for (id, d) in &self.data.drawings {
            ui.push_id(("drawing", id), |ui| {
                let active = self.sheets.view.as_ref().is_some_and(|v| v.id == *id);
                if ui
                    .add(egui::Button::selectable(active, &d.title).truncate())
                    .on_hover_text(id)
                    .clicked()
                    && !active
                {
                    self.sheets.view = Some(SheetView {
                        root: self.root.clone(),
                        id: id.clone(),
                        requested: None,
                        shown: None,
                        doc: None,
                        source: None,
                        error: None,
                        path: self.root.join(format!("{id}.pdf")).display().to_string(),
                        output: None,
                        zoom: 1.,
                        pan: Vec2::ZERO,
                    });
                    ui.ctx().request_repaint();
                }
            });
        }
        ui.separator();
    }
    pub(super) fn poll_drawings(&mut self, ctx: &egui::Context) {
        let completed = ready(&mut self.sheets.render);
        let exported = ready(&mut self.sheets.export);
        let version = self.sheets.version(self.data.revision);
        let Some(view) = &mut self.sheets.view else {
            return;
        };
        if view.root != self.root || !self.data.drawings.contains_key(&view.id) {
            self.sheets.view = None;
            return;
        }
        let key = Key {
            root: self.root.clone(),
            id: view.id.clone(),
            version,
        };
        if let Some((saved, r)) = completed
            && saved == key
        {
            view.requested = Some(key.version);
            match r {
                Ok((r, stamp, size)) => {
                    view.doc = Some(Arc::new(r));
                    view.source = Some((stamp, size));
                    view.shown = Some(key.version);
                    view.error = None;
                }
                Err(e) => {
                    view.error = Some(e);
                }
            }
        }
        if let Some((saved, r)) = exported
            && saved == key
        {
            view.output = Some(r.unwrap_or_else(|e| e));
        }
        if self.sheets.render.is_some() || view.requested == Some(key.version) {
            return;
        }
        view.requested = Some(key.version);
        view.error = None;
        view.output = None;
        let (tx, rx) = mpsc::channel();
        self.sheets.render = Some((key.clone(), rx));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_page(&key));
            ctx.request_repaint();
        });
    }
    pub(super) fn drawing_workspace(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) -> bool {
        if self.sheets.view.is_none() {
            return false;
        }
        let version = self.sheets.version(self.data.revision);
        let mut close = false;
        egui::Panel::left("drawing_browser")
            .default_size(230.)
            .size_range(168. ..=440.)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(ui, |ui| {
                // Clip the scroll content to the chosen panel width when a status line is long.
                let rect = ui.available_rect_before_wrap();
                let mut content = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                content.shrink_clip_rect(rect);
                ui.allocate_space(rect.size());
                egui::ScrollArea::vertical()
                    .id_salt("drawing_browser_scroll")
                    .show(&mut content, |ui| {
                        close = ui.button("Back to model").clicked();
                        ui.separator();
                        self.drawing_list(ui);
                        let v = self.sheets.view.as_mut().unwrap();
                        if ui.button("Fit sheet").clicked() {
                            v.pan = Vec2::ZERO;
                            v.zoom = 1.;
                        }
                        ui.separator();
                        let label = ui.label("Export path");
                        ui.add(
                            egui::TextEdit::singleline(&mut v.path)
                                .desired_width(ui.available_width()),
                        )
                        .labelled_by(label.id);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Browse…").clicked() {
                                self.dialog = Some(dialogs::drawing_file(
                                    frame,
                                    v.root.clone(),
                                    v.id.clone(),
                                ));
                            }
                            if ui
                                .add_enabled(
                                    self.sheets.export.is_none()
                                        && self.sheets.render.is_none()
                                        && v.error.is_none()
                                        && v.shown == Some(version)
                                        && !v.path.trim().is_empty(),
                                    egui::Button::new("Export"),
                                )
                                .clicked()
                            {
                                let key = Key {
                                    root: v.root.clone(),
                                    id: v.id.clone(),
                                    version,
                                };
                                let (Some(doc), Some((stamp, _))) = (v.doc.clone(), v.source)
                                else {
                                    return;
                                };
                                let path = PathBuf::from(v.path.trim());
                                v.output = None;
                                let (tx, rx) = mpsc::channel();
                                self.sheets.export = Some((key.clone(), rx));
                                let ctx = ui.ctx().clone();
                                std::thread::spawn(move || {
                                    let answer = export_page(&key.root, stamp, &doc, &path);
                                    let _ = tx.send(answer);
                                    ctx.request_repaint();
                                });
                            }
                        });
                        if let (Some(r), Some((_, size))) = (&v.doc, v.source) {
                            let paper = format!("{size:?}");
                            ui.label(format!(
                                "{paper} · {} × {} mm",
                                r.doc.size_mm[0], r.doc.size_mm[1]
                            ));
                        }
                        if let Some(rev) = v.shown {
                            ui.small(format!("Drawing revision {}", rev.revision));
                        }
                        if self.sheets.render.is_some() {
                            ui.label("Updating drawing…");
                        }
                        if let Some(e) = &v.error {
                            ui.colored_label(ui.visuals().error_fg_color, e);
                            if ui.button("Retry drawing").clicked() {
                                v.requested = None;
                            }
                        }
                        if self.sheets.export.is_some() {
                            ui.label("Exporting drawing…");
                        }
                        if let Some(message) = &v.output {
                            ui.label(message);
                        }
                        if let Some(r) = &v.doc {
                            for message in r.problems.iter().chain(&r.limits) {
                                ui.label(message);
                            }
                        }
                    });
            });
        if close {
            self.sheets.view = None;
            ui.ctx().request_repaint();
            return true;
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.sheets.view.as_mut().unwrap().canvas(ui));
        true
    }
}

impl SheetView {
    pub(super) fn paper_rect(&self, viewport: egui::Rect, size: [f32; 2]) -> egui::Rect {
        let [w, h] = size;
        let factor = ((viewport.width() - 32.).max(1.) / w)
            .min((viewport.height() - 32.).max(1.) / h)
            * self.zoom;
        egui::Rect::from_center_size(viewport.center() + self.pan, Vec2::new(w, h) * factor)
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Drawing sheet"));
        let rect = response.rect;
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Primary)
        {
            self.pan += response.drag_delta();
        }
        if response.hovered()
            && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0. {
                let zoom = (self.zoom * (wheel * 0.003).exp()).clamp(0.1, 20.);
                self.pan += (pos - rect.center() - self.pan) * (1. - zoom / self.zoom);
                self.zoom = zoom;
            }
        }
        painter.rect_filled(rect, 0., Color32::from_rgb(48, 54, 61));
        let Some(r) = &self.doc else { return };
        let [w, h] = r.doc.size_mm.map(|n| n as f32);
        let paper = self.paper_rect(rect, [w, h]);
        let factor = paper.width() / w;
        painter.rect_filled(paper, 0., Color32::WHITE);
        paint(
            &painter.with_clip_rect(rect.intersect(paper)),
            paper.left_bottom(),
            factor,
            &r.doc,
        );
    }
}
fn paint(p: &egui::Painter, origin: egui::Pos2, f: f32, doc: &VectorDoc) {
    let point = |v: [f64; 2]| origin + egui::vec2(v[0] as f32, -(v[1] as f32)) * f;
    for layer in &doc.layers {
        let [r, g, b] = layer.colour;
        let stroke = egui::Stroke::new(
            (layer.width_mm as f32 * f).max(0.6),
            Color32::from_rgb(r, g, b),
        );
        for path in &layer.paths {
            let points = match path {
                Path2::Polyline { points, closed } => {
                    let mut out: Vec<_> = points.iter().copied().map(point).collect();
                    if *closed && let Some(&first) = out.first() {
                        out.push(first);
                    }
                    out
                }
                Path2::Circle { centre, radius } if !layer.dashed => {
                    p.circle_stroke(point(*centre), *radius as f32 * f, stroke);
                    continue;
                }
                Path2::Arc {
                    centre,
                    radius,
                    start_deg,
                    end_deg,
                } => arc(
                    point(*centre),
                    radius * f64::from(f),
                    *start_deg,
                    (end_deg - start_deg).rem_euclid(360.),
                ),
                Path2::Circle { centre, radius } => {
                    arc(point(*centre), radius * f64::from(f), 0., 360.)
                }
            };
            if layer.dashed {
                p.extend(egui::Shape::dashed_line(
                    &points,
                    stroke,
                    (3. * f).max(2.),
                    (1.5 * f).max(1.),
                ));
            } else {
                p.add(egui::Shape::line(points, stroke));
            }
        }
    }
    for t in &doc.texts {
        let colour = doc
            .layers
            .iter()
            .find(|l| l.name == t.layer)
            .map_or(Color32::BLACK, |l| {
                Color32::from_rgb(l.colour[0], l.colour[1], l.colour[2])
            });
        let align = match t.anchor {
            Anchor::Start => egui::Align2::LEFT_BOTTOM,
            Anchor::Centre => egui::Align2::CENTER_BOTTOM,
            Anchor::End => egui::Align2::RIGHT_BOTTOM,
        };
        p.text(
            point(t.at),
            align,
            &t.text,
            egui::FontId::proportional((t.height_mm as f32 * f).clamp(0.5, 256.)),
            colour,
        );
    }
}
fn arc(centre: egui::Pos2, r: f64, start: f64, span: f64) -> Vec<egui::Pos2> {
    let limit = (1. - 0.2 / r.max(0.2)).acos() * 2.;
    let n = (span.to_radians() / limit).ceil().clamp(2., 2048.) as usize;
    (0..=n)
        .map(|i| {
            let (sin, cos) = (start + span * i as f64 / n as f64).to_radians().sin_cos();
            centre + egui::vec2((r * cos) as f32, (-r * sin) as f32)
        })
        .collect()
}

#[cfg(test)]
impl Sheets {
    pub(super) fn hold_render(&mut self, revision: u64) -> mpsc::Sender<Result<Page, String>> {
        let version = self.version(revision);
        let view = self.view.as_ref().unwrap();
        let (tx, rx) = mpsc::channel();
        self.render = Some((
            Key {
                root: view.root.clone(),
                id: view.id.clone(),
                version,
            },
            rx,
        ));
        tx
    }
}
