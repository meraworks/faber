//! Selection of one or more parts.
//!
//! `Shell::selected` is the primary part, the last one clicked.
//! `Shell::extra_selected` holds the other selected parts.

use super::move_tool::screen_point;
use super::*;
use glam::DVec3;

/// Least drag, in pixels, that draws a rectangle. A shorter drag is a click.
const BOX_MIN_PX: f32 = 4.;

impl Shell {
    /// Every selected part that exists and is visible, sorted by id.
    pub(super) fn selection(&self) -> Vec<String> {
        let mut ids = self.extra_selected.clone();
        ids.extend(self.selected.clone());
        ids.into_iter().filter(|id| self.visible(id)).collect()
    }

    fn visible(&self, id: &str) -> bool {
        self.data.parts.contains_key(id) && !self.hidden.contains(id)
    }

    /// Drops parts that are gone or hidden. A missing primary part gives way to an extra part.
    pub(super) fn prune_selection(&mut self) {
        let mut extra = std::mem::take(&mut self.extra_selected);
        extra.retain(|id| self.visible(id));
        self.extra_selected = extra;
        if !self.selected.as_deref().is_some_and(|id| self.visible(id)) {
            self.selected = self.extra_selected.pop_last();
        }
    }

    /// A click on the canvas at a screen point. Ctrl toggles the part under the pointer.
    /// A plain click selects that part, or clears the selection on empty space.
    pub(super) fn click_at(&mut self, rect: egui::Rect, pos: egui::Pos2, ctrl: bool) {
        let picked = self.pick_body(rect, pos);
        if ctrl {
            if let Some(id) = picked {
                self.toggle_part(&id);
            }
        } else {
            self.select_only(picked);
        }
        self.select_part_joint();
        self.mesh_dirty = true;
    }

    /// The other selected parts that move with a part. Empty when the part is not selected.
    pub(super) fn others_with(&self, id: &str) -> Vec<String> {
        let selection = self.selection();
        if !selection.iter().any(|part| part == id) {
            return Vec::new();
        }
        selection.into_iter().filter(|part| part != id).collect()
    }

    /// Selects one part. None clears the selection.
    pub(super) fn select_only(&mut self, id: Option<String>) {
        self.extra_selected.clear();
        self.selected = id;
    }

    /// Ctrl click. Adds the part, or removes it when it is selected.
    /// An added part becomes the primary part.
    pub(super) fn toggle_part(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            self.selected = self.extra_selected.pop_last();
        } else if !self.extra_selected.remove(id)
            && let Some(previous) = self.selected.replace(id.to_owned())
        {
            self.extra_selected.insert(previous);
        }
    }

    /// Shift click in the browser. Selects the parts from the primary part to this part.
    pub(super) fn select_range(&mut self, id: &str) {
        let order: Vec<String> = self.bodies.iter().map(|body| body.id.clone()).collect();
        let position = |part: &str| order.iter().position(|other| other.as_str() == part);
        let (Some(to), Some(from)) = (position(id), self.selected.as_deref().and_then(position))
        else {
            self.select_only(Some(id.to_owned()));
            return;
        };
        self.extra_selected = order[from.min(to)..=from.max(to)]
            .iter()
            .filter(|part| part.as_str() != id)
            .cloned()
            .collect();
        self.selected = Some(id.to_owned());
    }

    /// Selects these parts. The last id becomes the primary part.
    pub(super) fn select_many(&mut self, mut ids: Vec<String>) {
        self.selected = ids.pop();
        self.extra_selected = ids.into_iter().collect();
    }

    /// Selects every visible part.
    pub(super) fn select_visible(&mut self) {
        let ids = self
            .bodies
            .iter()
            .filter(|body| !self.hidden.contains(&body.id))
            .map(|body| body.id.clone())
            .collect();
        self.select_many(ids);
    }

    /// Escape clears the selection. Ctrl + A selects all visible parts.
    pub(super) fn selection_keys(&mut self, ctx: &egui::Context) {
        self.prune_selection();
        if self.move_draft.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (escape, select_all) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::A),
            )
        });
        if escape {
            self.select_only(None);
        }
        if select_all {
            self.select_visible();
        }
    }

    /// Left drag on empty space draws a rectangle. Returns true while the rectangle runs,
    /// and when it has selected parts. A busy canvas does not start a rectangle.
    pub(super) fn box_select(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        busy: bool,
        over_canvas: bool,
    ) -> bool {
        let (down, pressed, origin, now, ctrl) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_pressed(),
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
                i.modifiers.command || i.modifiers.ctrl,
            )
        });
        let Some(start) = self.box_drag else {
            // The canvas must be the top widget under the pointer. A window over it takes the press.
            if pressed
                && !busy
                && over_canvas
                && let Some(origin) = origin
                && rect.contains(origin)
                && self.pick_body(rect, origin).is_none()
            {
                self.box_drag = Some(origin);
            }
            return false;
        };
        let current = now.unwrap_or(start);
        if down {
            let area = egui::Rect::from_two_pos(start, current);
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(area, 0., Color32::from_rgba_unmultiplied(80, 150, 255, 36));
            painter.rect_stroke(
                area,
                0.,
                egui::Stroke::new(1., Color32::from_rgb(120, 180, 255)),
                egui::StrokeKind::Inside,
            );
            return true;
        }
        self.box_drag = None;
        if (current - start).length() < BOX_MIN_PX {
            // A short drag is a click. egui reports it as a drag, so the canvas click does not run.
            self.click_at(rect, current, ctrl);
            return true;
        }
        self.select_in_box(
            rect,
            egui::Rect::from_two_pos(start, current),
            current.x >= start.x,
        );
        true
    }

    /// Selects the visible parts that the screen area holds.
    /// Left to right needs a part inside the area. Right to left needs a part that the area touches.
    fn select_in_box(&mut self, canvas: egui::Rect, area: egui::Rect, left_to_right: bool) {
        let matrix = self.camera.matrix(canvas.aspect_ratio());
        let mut ids = Vec::new();
        for body in self.bodies.iter() {
            if self.hidden.contains(&body.id) {
                continue;
            }
            let mut screen = egui::Rect::NOTHING;
            for corner in 0..8 {
                let point = DVec3::from(std::array::from_fn(|axis| {
                    body.bounds[(corner >> axis) & 1][axis]
                }));
                if let Some(pos) = screen_point(canvas, matrix, point) {
                    screen.extend_with(pos);
                }
            }
            if screen == egui::Rect::NOTHING {
                continue;
            }
            let hit = if left_to_right {
                area.contains(screen.min) && area.contains(screen.max)
            } else {
                area.intersects(screen)
            };
            if hit {
                ids.push(body.id.clone());
            }
        }
        ids.sort();
        self.select_many(ids);
    }
}
