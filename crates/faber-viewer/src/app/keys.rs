use super::*;
use command_search::search_commands;
use egui::{InputState, Key};

#[derive(Clone, Copy)]
pub(crate) enum Command {
    Undo,
    Redo,
    Delete,
    Fit,
    Move,
    Search,
    ToggleSection,
    ToggleStrict,
    Escape,
    Apply,
    SelectAll,
    Checks,
    Export,
    Import,
    Sketch,
    Top,
    Front,
    Iso,
    Project(bool),
    ExactPlacement,
    PointToPoint,
    Align,
    EditFeatures,
    CopyPlacement,
    PastePlacement,
    Hide,
    Isolate,
    ShowAll,
    Parameters,
    Feature(ToolKind),
}

pub(crate) fn command_for(
    input: &InputState,
    text_focus: bool,
    window_open: bool,
) -> Option<Command> {
    input.events.iter().find_map(|event| {
        let egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } = event
        else {
            return None;
        };
        let ctrl = modifiers.ctrl || modifiers.command;
        if modifiers.alt || modifiers.mac_cmd && !modifiers.command {
            return None;
        }
        match (*key, ctrl, modifiers.shift) {
            (Key::Escape, false, false) => Some(Command::Escape),
            (Key::Enter, false, false) => Some(Command::Apply),
            _ if text_focus => None,
            (Key::Z, true, false) => Some(Command::Undo),
            (Key::Y, true, false) | (Key::Z, true, true) => Some(Command::Redo),
            _ if window_open => None,
            (Key::Delete, false, false) => Some(Command::Delete),
            (Key::F, false, false) => Some(Command::Fit),
            (Key::M, false, false) => Some(Command::Move),
            (Key::S, false, false) => Some(Command::Search),
            (Key::S, false, true) => Some(Command::ToggleSection),
            (Key::A, true, false) => Some(Command::SelectAll),
            _ => None,
        }
    })
}

impl Shell {
    pub(super) fn shortcut(&mut self, ctx: &egui::Context) {
        self.prune_selection();
        let text_focus = ctx.text_edit_focused();
        let command = ctx.input(|i| command_for(i, text_focus, self.window.open.is_some()));
        let Some(command) = command else { return };
        if egui::Popup::is_any_open(ctx) {
            if matches!(command, Command::Escape) {
                egui::Popup::close_all(ctx);
            }
            return;
        }
        if self.search.open {
            match command {
                Command::Escape => self.search.open = false,
                Command::Apply => {
                    let entry = search_commands(&self.search.query)
                        .into_iter()
                        .find(|e| self.command_enabled(e.command));
                    if let Some(entry) = entry {
                        self.search.open = false;
                        self.run_command(ctx, entry.command);
                    }
                }
                _ => (),
            }
        } else if self.window.next.is_some() {
            if matches!(command, Command::Escape) {
                self.window.next = None;
            }
        } else if self.project_loaded || self.window.open.is_some() {
            self.run_command(ctx, command);
        }
    }

    pub(super) fn command_enabled(&self, command: Command) -> bool {
        use Command::*;
        let idle = self.pending.is_none();
        let selection = self.selected.is_some();
        match command {
            Undo | Redo | Import | Sketch | Project(_) => idle,
            Move | Delete | ExactPlacement | Align | PointToPoint => idle && selection,
            CopyPlacement | Hide | Isolate => selection,
            EditFeatures => self
                .selected
                .as_ref()
                .and_then(|id| self.data.parts.get(id))
                .is_some_and(|p| matches!(p.geometry, faber_model::Geometry::History { .. })),
            PastePlacement => {
                idle && self.copied_placement.as_ref().is_some_and(|(source, _)| {
                    self.data.parts.contains_key(source)
                        && self
                            .selected
                            .as_ref()
                            .is_none_or(|target| self.data.parts.contains_key(target))
                })
            }
            Feature(_) => idle && selection,
            ShowAll => !self.hidden.is_empty(),
            Parameters => idle,
            _ => true,
        }
    }

    pub(super) fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        use Command::*;
        if !self.command_enabled(command) {
            return;
        }
        let selected = self.selected.clone();
        match command {
            Escape => {
                if !self.stop_pick() {
                    if self.window.open.is_some() {
                        self.cancel_window();
                    } else {
                        self.select_only(None);
                    }
                }
            }
            Apply => {
                if self.window.open.is_some() && !self.window.stale {
                    self.window.apply = true;
                }
            }
            Undo | Redo => self.move_history(ctx, matches!(command, Redo)),
            Delete => {
                let ids = self.selection();
                if self.pending.is_none() && !ids.is_empty() {
                    let names = ids
                        .iter()
                        .map(|id| self.data.parts[id].name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.deleting = Some(names);
                    self.save_sections(
                        ctx,
                        ids.into_iter()
                            .map(|id| faber_model::Edit::RemovePart { id })
                            .collect(),
                    );
                }
            }
            Fit => self.fit(),
            Move | Align | PointToPoint | ExactPlacement => {
                if let Some(id) = selected {
                    match command {
                        ExactPlacement => self.start_placement(&id),
                        Align => self.start_move_in(&id, move_tool::MoveMode::Align),
                        PointToPoint => self.start_move_in(&id, move_tool::MoveMode::PointToPoint),
                        _ => self.start_move(&id),
                    }
                }
            }
            Search => {
                self.search.open = true;
                self.search.query.clear();
                self.search.position = ctx
                    .pointer_hover_pos()
                    .unwrap_or(ctx.content_rect().center());
                self.search.focus = true;
            }
            ToggleSection => {
                self.section_on = !self.section_on;
                self.mesh_dirty = true;
            }
            ToggleStrict => self.strict_edits = !self.strict_edits,
            SelectAll => self.select_visible(),
            Checks => self.request_window(Draft::Checks),
            Export => self.request_window(Draft::Export),
            Import => self.request_window(Draft::Import(ImportDraft {
                bought: false,
                source: String::new(),
                licence: String::new(),
                designation: String::new(),
                expected: self.data.clone(),
                path: String::new(),
                name: "Imported part".into(),
            })),
            Sketch => self.request_window(Draft::Sketch(crate::sketch::SketchDraft::new(
                self.data.clone(),
            ))),
            Top | Front | Iso => {
                self.camera.set_view(match command {
                    Top => faber_scene::View::Top,
                    Front => faber_scene::View::Front,
                    _ => faber_scene::View::Iso,
                });
                if matches!(command, Iso) {
                    self.fit();
                }
                self.dirty = true;
            }
            Project(create) => self.request_window(Draft::Project(ProjectDraft {
                create,
                path: String::new(),
                name: "Untitled".into(),
            })),
            EditFeatures => self.reveal_features = true,
            CopyPlacement => {
                if let Some(id) = selected {
                    self.copied_placement =
                        Some((id.clone(), self.data.parts[&id].placement.clone()));
                }
            }
            PastePlacement => {
                if let Some((source, placement)) = &self.copied_placement {
                    let id = selected.as_ref().unwrap_or(source);
                    if let Some(mut part) = self.data.parts.get(id).cloned() {
                        part.placement = placement.clone();
                        self.save_sections(
                            ctx,
                            vec![faber_model::Edit::PutPart {
                                id: id.clone(),
                                part,
                            }],
                        );
                    }
                }
            }
            Hide => {
                let ids = self.selection();
                self.hidden.extend(ids);
                self.mesh_dirty = true;
            }
            Isolate => {
                let ids = self.selection();
                self.hidden = self
                    .bodies
                    .iter()
                    .filter(|b| !ids.contains(&b.id))
                    .map(|b| b.id.clone())
                    .collect();
                self.mesh_dirty = true;
            }
            ShowAll => {
                self.hidden.clear();
                self.mesh_dirty = true;
            }
            Parameters => self.request_window(Draft::Parameters(
                self.data.clone(),
                self.data.params.clone(),
            )),
            Feature(kind) => {
                if let Some(id) = selected
                    && let Err(error) = self.new_feature(&id, kind)
                {
                    self.error = Some(error);
                }
            }
        }
    }
}
