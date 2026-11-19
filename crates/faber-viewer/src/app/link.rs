//! Applies display commands from the MCP server and reports what the viewer shows.
//! See `faber_wiring::viewer_link` for the files.

use super::*;
use faber_wiring::viewer_link::{self, PickedItem, ViewDirection, ViewerAction, ViewerState};

#[derive(Default)]
pub(super) struct Link {
    started: bool,
    stamp: Option<SystemTime>,
    applied: u64,
    notices: Vec<String>,
    written: Option<ViewerState>,
}

impl Shell {
    /// Applies a new command and writes the display state when it changed.
    pub(super) fn poll_link(&mut self, ctx: &egui::Context) {
        if !self.project_loaded {
            return;
        }
        if !self.link.started {
            // A command from before this viewer opened the project is old. Skip it.
            self.link = Link {
                started: true,
                stamp: viewer_link::command_stamp(&self.root),
                applied: viewer_link::read_command(&self.root).map_or(0, |c| c.seq),
                ..Default::default()
            };
        } else {
            let stamp = viewer_link::command_stamp(&self.root);
            if stamp != self.link.stamp {
                self.link.stamp = stamp;
                if let Some(command) = viewer_link::read_command(&self.root)
                    && command.seq > self.link.applied
                {
                    self.link.notices.clear();
                    for action in command.actions {
                        self.apply_link_action(ctx, action);
                    }
                    self.link.applied = command.seq;
                    self.dirty = true;
                    ctx.request_repaint();
                }
            }
        }
        let state = self.link_state();
        if self.link.written.as_ref() != Some(&state)
            && viewer_link::write_state(&self.root, &state).is_ok()
        {
            self.link.written = Some(state);
        }
    }

    /// Forgets the link state. The next poll starts again for the new project.
    pub(super) fn reset_link(&mut self) {
        self.link = Link::default();
    }

    fn link_state(&self) -> ViewerState {
        ViewerState {
            applied: self.link.applied,
            notices: self.link.notices.clone(),
            revision: self.data.revision,
            hidden: self.hidden.iter().cloned().collect(),
            selected: self.selection(),
            picked: self
                .picks
                .iter()
                .filter_map(|pick| match pick {
                    Pick::Face { part, face } => Some(PickedItem {
                        part: part.clone(),
                        kind: "face".into(),
                        index: *face,
                    }),
                    Pick::Edge { part, edge } => Some(PickedItem {
                        part: part.clone(),
                        kind: "edge".into(),
                        index: *edge,
                    }),
                    Pick::Body(_) => None,
                })
                .collect(),
            section: self.section_selected.clone(),
            section_on: self.section_on,
        }
    }

    /// Keeps the parts that exist and adds a notice for each other part.
    fn known_parts(&mut self, parts: Vec<String>) -> Vec<String> {
        let (known, missing): (Vec<_>, Vec<_>) = parts
            .into_iter()
            .partition(|id| self.data.parts.contains_key(id));
        for id in missing {
            self.link.notices.push(format!("Part {id} was not found."));
        }
        known
    }

    fn apply_link_action(&mut self, ctx: &egui::Context, action: ViewerAction) {
        match action {
            ViewerAction::Hide { parts } => {
                let parts = self.known_parts(parts);
                self.hidden.extend(parts);
                self.prune_selection();
                self.mesh_dirty = true;
            }
            ViewerAction::Show { parts } => {
                for id in self.known_parts(parts) {
                    self.hidden.remove(&id);
                }
                self.mesh_dirty = true;
            }
            ViewerAction::Isolate { parts } => {
                let keep = self.known_parts(parts);
                self.structure.clear_isolation();
                self.hidden = self
                    .data
                    .parts
                    .keys()
                    .filter(|id| !keep.contains(id))
                    .cloned()
                    .collect();
                self.prune_selection();
                self.mesh_dirty = true;
            }
            ViewerAction::ShowAll => {
                self.structure.clear_isolation();
                self.hidden.clear();
                self.mesh_dirty = true;
            }
            ViewerAction::Select { parts } => {
                let mut parts = self.known_parts(parts).into_iter();
                self.select_only(parts.next());
                self.extra_selected.extend(parts);
                self.mesh_dirty = true;
            }
            ViewerAction::Camera { view, parts } => {
                if let Some(view) = view {
                    self.camera.set_view(scene_view(view));
                }
                let parts = self.known_parts(parts);
                self.frame_parts(&parts);
            }
            ViewerAction::Section { id: Some(id) } => {
                if self.data.sections.contains_key(&id) {
                    self.views.section_override = Some(id.clone());
                    self.section_selected = Some(id);
                    self.sync_section(ctx);
                    self.mesh_dirty = true;
                } else {
                    self.link
                        .notices
                        .push(format!("Section {id} was not found."));
                }
            }
            ViewerAction::Section { id: None } => {
                self.views.section_override = None;
                self.section_selected = None;
                self.section_on = false;
                self.section_scope.clear();
                self.mesh_dirty = true;
            }
            ViewerAction::ApplyView { name } => {
                if let Err(error) = self.apply_named_view(ctx, &name) {
                    self.link.notices.push(error);
                }
            }
        }
    }

    /// Frames these parts, or all visible parts when the list is empty.
    fn frame_parts(&mut self, parts: &[String]) {
        let mut lo = glam::Vec3::splat(f32::INFINITY);
        let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;
        for body in self.bodies.iter().filter(|b| {
            if parts.is_empty() {
                !self.hidden.contains(&b.id)
            } else {
                parts.contains(&b.id)
            }
        }) {
            lo = lo.min(glam::Vec3::from(body.bounds[0].map(|v| v as f32)));
            hi = hi.max(glam::Vec3::from(body.bounds[1].map(|v| v as f32)));
            any = true;
        }
        if !any {
            self.fit();
            return;
        }
        self.set_frame((lo + hi) / 2., (hi - lo).length().max(1.) * 1.8);
    }
}

fn scene_view(view: ViewDirection) -> faber_scene::View {
    match view {
        ViewDirection::Iso => faber_scene::View::Iso,
        ViewDirection::Front => faber_scene::View::Front,
        ViewDirection::Back => faber_scene::View::Back,
        ViewDirection::Left => faber_scene::View::Left,
        ViewDirection::Right => faber_scene::View::Right,
        ViewDirection::Top => faber_scene::View::Top,
        ViewDirection::Bottom => faber_scene::View::Bottom,
    }
}
