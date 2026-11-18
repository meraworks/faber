use super::*;
use keys::Command;

#[derive(Default)]
pub(crate) struct CommandSearch {
    pub query: String,
    pub open: bool,
    pub(super) position: egui::Pos2,
    pub(super) focus: bool,
}

pub(crate) struct CommandEntry {
    pub name: &'static str,
    pub shortcut: &'static str,
    pub command: Command,
}

const fn entry(name: &'static str, shortcut: &'static str, command: Command) -> CommandEntry {
    CommandEntry {
        name,
        shortcut,
        command,
    }
}
const COMMANDS: &[CommandEntry] = &[
    entry("Fit view", "F", Command::Fit),
    entry("Move/Copy", "M", Command::Move),
    entry("Section analysis", "Shift+S", Command::ToggleSection),
    entry("Undo", "Ctrl+Z", Command::Undo),
    entry("Redo", "Ctrl+Y", Command::Redo),
    entry("Delete parts", "Delete", Command::Delete),
    entry("Select all", "Ctrl+A", Command::SelectAll),
    entry("Top", "", Command::Top),
    entry("Front", "", Command::Front),
    entry("Home", "", Command::Iso),
    entry("Strict mode", "", Command::ToggleStrict),
    entry("Iso", "", Command::Iso),
    entry("Checks", "", Command::Checks),
    entry("Export", "", Command::Export),
    entry("Import model", "", Command::Import),
    entry("New sketch", "", Command::Sketch),
    entry("New project", "", Command::Project(true)),
    entry("Open project", "", Command::Project(false)),
    entry("Exact placement", "", Command::ExactPlacement),
    entry("Point to point", "", Command::PointToPoint),
    entry("Align to", "", Command::Align),
    entry("Edit features", "", Command::EditFeatures),
    entry("Copy placement", "", Command::CopyPlacement),
    entry("Paste placement", "", Command::PastePlacement),
    entry("Hide bodies", "", Command::Hide),
    entry("Isolate bodies", "", Command::Isolate),
    entry("Show all bodies", "", Command::ShowAll),
    entry("Edit parameters", "", Command::Parameters),
    entry("Linear pattern", "", Command::Feature(ToolKind::Linear)),
    entry("Circular pattern", "", Command::Feature(ToolKind::Circular)),
    entry("Mirror", "", Command::Feature(ToolKind::Mirror)),
    entry("Shell", "", Command::Feature(ToolKind::Shell)),
    entry("Scale", "", Command::Feature(ToolKind::Scale)),
    entry("Pipe", "", Command::Feature(ToolKind::Pipe)),
    entry("Path pattern", "", Command::Feature(ToolKind::PathPattern)),
    entry("Fillet", "", Command::Feature(ToolKind::Fillet)),
    entry("Chamfer", "", Command::Feature(ToolKind::Chamfer)),
    entry("Offset solid", "", Command::Feature(ToolKind::Offset)),
    entry("Draft", "", Command::Feature(ToolKind::Draft)),
    entry("Thicken face", "", Command::Feature(ToolKind::Thicken)),
    entry("Hole", "", Command::Feature(ToolKind::Hole)),
    entry("Thread", "", Command::Feature(ToolKind::Thread)),
    entry("Split body", "", Command::Feature(ToolKind::Split)),
    entry("Reference plane", "", Command::Feature(ToolKind::Plane)),
    entry("Plane on face", "", Command::Feature(ToolKind::FacePlane)),
    entry("Reference axis", "", Command::Feature(ToolKind::Axis)),
    entry("Reference point", "", Command::Feature(ToolKind::Point)),
    entry("Plane at angle", "", Command::Feature(ToolKind::AnglePlane)),
    entry("Midplane", "", Command::Feature(ToolKind::Midplane)),
    entry(
        "Plane through points",
        "",
        Command::Feature(ToolKind::PointsPlane),
    ),
    entry(
        "Axis from cylinder",
        "",
        Command::Feature(ToolKind::CylinderAxis),
    ),
    entry(
        "Axis from planes",
        "",
        Command::Feature(ToolKind::PlanesAxis),
    ),
    entry(
        "Point at centre",
        "",
        Command::Feature(ToolKind::CentrePoint),
    ),
];

pub(crate) fn search_commands(query: &str) -> Vec<&'static CommandEntry> {
    let query = query.trim().to_lowercase();
    COMMANDS
        .iter()
        .filter(|entry| entry.name.to_lowercase().contains(&query))
        .collect()
}

impl Shell {
    pub(super) fn command_search(&mut self, ctx: &egui::Context) {
        if !self.search.open {
            return;
        }
        let mut picked = None;
        let shown = egui::Area::new(egui::Id::unique("command_search"))
            .order(egui::Order::Foreground)
            .fixed_pos(self.search.position)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(260.);
                    let label = ui.label("Command search");
                    let edit = ui
                        .text_edit_singleline(&mut self.search.query)
                        .labelled_by(label.id);
                    if std::mem::take(&mut self.search.focus) {
                        edit.request_focus();
                    }
                    egui::ScrollArea::vertical()
                        .max_height(260.)
                        .show(ui, |ui| {
                            for entry in search_commands(&self.search.query) {
                                if ui
                                    .add_enabled(
                                        self.command_enabled(entry.command),
                                        egui::Button::new(entry.name).shortcut_text(entry.shortcut),
                                    )
                                    .clicked()
                                {
                                    picked = Some(entry.command);
                                }
                            }
                        });
                });
            });
        if let Some(command) = picked {
            self.search.open = false;
            self.run_command(ctx, command);
        } else if shown.response.clicked_elsewhere() {
            self.search.open = false;
        }
    }
}
