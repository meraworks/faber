use super::*;
use std::path::Path;

impl Panel {
    pub(in crate::app) fn file_chosen(&mut self, id: &str, path: PathBuf) {
        if self.part == id {
            self.path = path.display().to_string();
        }
    }
    pub(super) fn pattern_controls(&mut self, ui: &mut egui::Ui) -> Action {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.bends, "Show bend lines");
            if ui.button("Fit pattern").clicked() {
                self.zoom = 1.;
                self.pan = Vec2::ZERO;
            }
        });
        form::field(ui, "Flat pattern file", &mut self.path);
        let enabled = self.current
            && self
                .result
                .as_ref()
                .is_some_and(|r| r.stamp.is_some() && r.pattern.problems.is_empty())
            && self.export.is_none()
            && !self.path.trim().is_empty();
        let action = ui
            .horizontal_wrapped(|ui| {
                if ui.button("Browse…").clicked() {
                    return Action::Browse;
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Export flat pattern"))
                    .on_hover_text("Write a DXF or SVG file. Both files include the bend lines.")
                    .clicked()
                {
                    Action::Export
                } else {
                    Action::None
                }
            })
            .inner;
        if let Some(r) = &self.result {
            let f = &r.pattern;
            ui.label(format!("{:.2} × {:.2} mm", f.size_mm[0], f.size_mm[1]));
            for b in &f.bends {
                let direction = match b.direction {
                    metal::Direction::Up => "up",
                    metal::Direction::Down => "down",
                };
                ui.label(format!(
                    "{} · {}° {direction} · allowance {:.2} mm",
                    b.flange,
                    b.angle_deg.abs(),
                    b.allowance
                ));
            }
        }
        action
    }
    pub(super) fn export(&mut self, app: &Shell, ctx: &egui::Context) {
        let Some(r) = self.result.clone() else {
            return;
        };
        let root = app.root.clone();
        let id = self.part.clone();
        let path = PathBuf::from(self.path.trim());
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.export = Some(rx);
        self.message = None;
        std::thread::spawn(move || {
            let result = (|| {
                let ext = path
                    .extension()
                    .and_then(|x| x.to_str())
                    .filter(|x| matches!(x.to_ascii_lowercase().as_str(), "dxf" | "svg"))
                    .ok_or("Use .dxf or .svg for a flat pattern.")?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                let tmp = tempfile::Builder::new()
                    .suffix(&format!(".{ext}"))
                    .tempfile_in(parent)
                    .map_err(|e| e.to_string())?
                    .into_temp_path();
                metal::export(&r.pattern, &id, &tmp)?;
                if Some(faber_wiring::project_fingerprint(&root)?) != r.stamp {
                    return Err("The sheet changed. Export the current pattern.".to_owned());
                }
                tmp.persist(&path).map_err(|e| e.to_string())?;
                Ok(format!("Exported {}.", path.display()))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl Shell {
    pub(in crate::app) fn sheet_canvas(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(p) = self.sheet_metal.as_mut().filter(|p| p.flat) else {
            return false;
        };
        let (r, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        r.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Flat pattern viewport")
        });
        let rect = r.rect;
        painter.rect_filled(rect, 0., Color32::from_rgb(237, 241, 243));
        if r.dragged_by(egui::PointerButton::Middle) || r.dragged_by(egui::PointerButton::Primary) {
            p.pan += r.drag_delta();
        }
        if r.hovered()
            && let Some(pos) = r.hover_pos()
        {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0. {
                let zoom = (p.zoom * (wheel * 0.003).exp()).clamp(0.1, 50.);
                p.pan += (pos - rect.center() - p.pan) * (1. - zoom / p.zoom);
                p.zoom = zoom;
            }
        }
        let Some(data) = &p.result else {
            let text = if p.read.is_some() {
                "Reading flat pattern…"
            } else {
                "Flat pattern unavailable"
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(16.),
                Color32::DARK_GRAY,
            );
            return true;
        };
        let flat = &data.pattern;
        let min = flat
            .outline
            .iter()
            .flatten()
            .fold([f64::INFINITY; 2], |[x, y], q| [x.min(q[0]), y.min(q[1])]);
        let [w, h] = flat.size_mm;
        let scale = ((rect.width() - 48.).max(1.) / w.max(1e-6) as f32)
            .min((rect.height() - 48.).max(1.) / h.max(1e-6) as f32)
            * p.zoom;
        let screen = |q: [f64; 2]| {
            rect.center()
                + p.pan
                + egui::vec2(
                    (q[0] - min[0] - w / 2.) as f32,
                    (min[1] + h / 2. - q[1]) as f32,
                ) * scale
        };
        let stroke = egui::Stroke::new(1.8, Color32::from_rgb(37, 65, 76));
        for loop_ in flat.outline.iter().chain(&flat.holes) {
            painter.add(egui::Shape::closed_line(
                loop_.iter().copied().map(screen).collect(),
                stroke,
            ));
        }
        if p.bends {
            for bend in &flat.bends {
                painter.extend(egui::Shape::dashed_line(
                    &bend.line.map(screen),
                    egui::Stroke::new(1.8, Color32::from_rgb(26, 121, 157)),
                    8.,
                    5.,
                ));
            }
        }
        true
    }
}
