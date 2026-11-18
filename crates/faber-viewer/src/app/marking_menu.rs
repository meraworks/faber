//! Canvas menu. A short right click keeps it open. A drag chooses a sector.

use super::*;
use keys::Command;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

const RADIUS: f32 = 121.;
const DEAD_ZONE: f32 = 21.;

#[derive(Default)]
pub(super) struct MarkingMenu {
    pub(super) anchor: Option<egui::Pos2>,
    pub(super) last: Option<Command>,
    held: bool,
}

const ENTRIES: [(&str, Option<Command>); 8] = [
    ("Repeat last command", None),
    ("Press pull", Some(Command::PressPull)),
    ("Undo", Some(Command::Undo)),
    ("Redo", Some(Command::Redo)),
    ("Move/Copy", Some(Command::Move)),
    ("Hole", Some(Command::Feature(ToolKind::Hole))),
    ("New sketch", Some(Command::Sketch)),
    ("Delete", Some(Command::Delete)),
];

fn direction(index: usize) -> Vec2 {
    Vec2::angled(index as f32 * FRAC_PI_4 - FRAC_PI_2)
}

fn sector(delta: Vec2) -> Option<usize> {
    (delta.length_sq() >= DEAD_ZONE * DEAD_ZONE)
        .then(|| ((delta.y.atan2(delta.x) + FRAC_PI_2) / FRAC_PI_4).round() as i32)
        .map(|i| i.rem_euclid(8) as usize)
}

fn draw_ring(
    ui: &egui::Ui,
    centre: egui::Pos2,
    radius: f32,
    enabled: [bool; 8],
    active: Option<usize>,
) {
    let mut mesh = egui::Mesh::default();
    for (index, enabled) in enabled.into_iter().enumerate() {
        let fill = match (enabled, active == Some(index)) {
            (true, true) => Color32::from_rgb(43, 111, 113),
            (true, false) => Color32::from_rgb(37, 52, 64),
            _ => Color32::from_rgb(34, 40, 48),
        };
        let angle = index as f32 * FRAC_PI_4 - FRAC_PI_2;
        for step in 0..=8 {
            let ray = Vec2::angled(angle + (step as f32 / 8. - 0.5) * (FRAC_PI_4 - 0.025));
            let n = mesh.vertices.len() as u32;
            mesh.colored_vertex(centre + ray * DEAD_ZONE, fill);
            mesh.colored_vertex(centre + ray * radius, fill);
            if step > 0 {
                mesh.add_triangle(n - 2, n - 1, n + 1);
                mesh.add_triangle(n - 2, n + 1, n);
            }
        }
    }
    ui.painter().add(mesh);
    ui.painter().circle_filled(centre, DEAD_ZONE - 2., PANEL);
    ui.painter()
        .circle_stroke(centre, DEAD_ZONE - 2., egui::Stroke::new(1., MUTED));
}

fn sector_button(
    ui: &mut egui::Ui,
    index: usize,
    label: &str,
    centre: egui::Pos2,
    radius: f32,
    enabled: bool,
) -> bool {
    let position = centre + direction(index) * (radius * 0.7);
    let rect = egui::Rect::from_center_size(position, egui::vec2(radius * 0.64, 32.));
    let mut builder = egui::UiBuilder::new().max_rect(rect);
    if !enabled {
        builder = builder.disabled();
    }
    // The label belongs inside the allocated ring. Keep the menu cursor below it.
    let child = ui.new_child(builder);
    let response = child.interact(rect, child.scope_id().with(index), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let text = if index == 0 {
        "Repeat last\ncommand"
    } else {
        label
    };
    ui.painter().text(
        position,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.5),
        if enabled { Color32::WHITE } else { MUTED },
    );
    enabled && response.clicked()
}

impl Shell {
    pub(super) fn start_mark(&mut self, rect: egui::Rect, point: egui::Pos2) {
        self.pick_canvas(rect, point, false, false, false, true);
        self.marking.anchor = Some(point);
        self.marking.held = true;
    }

    fn mark_command(&self, index: usize) -> Option<Command> {
        let command = ENTRIES[index].1.or(self.marking.last)?;
        self.command_enabled(command).then_some(command)
    }

    fn run_mark(&mut self, ctx: &egui::Context, index: usize) {
        self.marking.anchor = None;
        if let Some(command) = self.mark_command(index) {
            self.run_command(ctx, command);
        }
    }

    pub(super) fn marking_menu(&mut self, ctx: &egui::Context) {
        let Some(origin) = self.marking.anchor else {
            return;
        };
        let (pointer, released) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.button_released(egui::PointerButton::Secondary),
            )
        });
        let gesture = pointer.and_then(|p| sector(p - origin));
        if self.marking.held && released {
            self.marking.held = false;
            if let Some(index) = gesture {
                self.run_mark(ctx, index);
                return;
            }
        }
        let screen = ctx.content_rect().shrink(5.);
        let radius = RADIUS.min(screen.width() * 0.5).min(screen.height() * 0.35);
        let size = egui::Vec2::splat(radius * 2.);
        let context_height = if self.selected.is_some() { 248. } else { 142. };
        let height = (size.y + context_height).min(screen.height());
        let top = egui::pos2(
            (origin.x - radius).clamp(screen.left(), (screen.right() - size.x).max(screen.left())),
            (origin.y - radius).clamp(screen.top(), (screen.bottom() - height).max(screen.top())),
        );
        let mut picked = None;
        let shown = egui::Area::new(egui::Id::unique("canvas_marking_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(top)
            .movable(false)
            .show(ctx, |ui| {
                let (ring, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let centre = ring.center();
                let active = if self.marking.held {
                    gesture
                } else {
                    pointer
                        .filter(|p| p.distance(centre) <= radius)
                        .and_then(|p| sector(p - centre))
                };
                let enabled = std::array::from_fn(|i| self.mark_command(i).is_some());
                draw_ring(ui, centre, radius, enabled, active);
                for (index, (label, _)) in ENTRIES.iter().enumerate() {
                    if sector_button(ui, index, label, centre, radius, enabled[index]) {
                        picked = Some(index);
                    }
                }
                if !self.marking.held
                    && ctx.input(|i| i.pointer.primary_clicked())
                    && let Some(index) = active
                    && self.mark_command(index).is_some()
                {
                    picked = Some(index);
                }
                let context = ui.scope_builder(egui::UiBuilder::new().closable(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width((size.x - 16.).max(40.));
                        egui::ScrollArea::vertical()
                            .id_salt("marking_context")
                            .max_height(
                                (screen.bottom() - ui.cursor().top() - 10.)
                                    .clamp(30., context_height),
                            )
                            .show(ui, |ui| self.body_menu(ui));
                    });
                });
                context.response.should_close()
            });
        if self.marking.anchor.is_none() {
            return;
        }
        if let Some(index) = picked {
            self.run_mark(ctx, index);
        } else if shown.inner
            || (!self.marking.held
                && ctx.input(|i| i.pointer.primary_pressed())
                && pointer.is_some_and(|p| !shown.response.rect.contains(p)))
        {
            self.marking.anchor = None;
        }
    }
}
