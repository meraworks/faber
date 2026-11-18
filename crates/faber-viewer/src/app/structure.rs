//! Component browser and bill of materials.
use super::*;
use faber_core::{Operation, PartInfo, bom::BomRow};
use std::sync::Arc;

#[derive(Default)]
struct Tree {
    children: BTreeMap<String, Vec<String>>,
    parts: BTreeMap<String, Vec<usize>>,
}
impl Tree {
    fn new(data: &ProjectData, bodies: &[Body]) -> Self {
        let mut t = Self::default();
        if data.components.is_empty() {
            return t;
        }
        for (id, c) in &data.components {
            t.children
                .entry(c.parent.clone().unwrap_or_default())
                .or_default()
                .push(id.clone());
        }
        for (index, b) in bodies.iter().enumerate() {
            if let Some(p) = data.parts.get(&b.id) {
                t.parts
                    .entry(p.component.clone().unwrap_or_default())
                    .or_default()
                    .push(index);
            }
        }
        t
    }
}

#[derive(Default)]
pub(super) struct Browser {
    tree: Arc<Tree>,
    pub(super) selected: Option<String>,
    closed: BTreeSet<String>,
    isolated: Option<(String, BTreeSet<String>)>,
    pub(super) bom_open: bool,
    generation: u64,
    bom_rows: Option<(u64, Vec<BomRow>)>,
    bom_error: Option<&'static str>,
    bom_job: Option<(u64, mpsc::Receiver<Vec<BomRow>>)>,
}
impl Browser {
    pub(super) fn new(data: &ProjectData, bodies: &[Body]) -> Self {
        Self {
            tree: Arc::new(Tree::new(data, bodies)),
            ..Self::default()
        }
    }
    pub(super) fn clear_isolation(&mut self) {
        self.isolated = None;
    }
    pub(super) fn reload(&mut self, data: &ProjectData, bodies: &[Body], changed_project: bool) {
        if changed_project {
            *self = Self::new(data, bodies);
            return;
        }
        self.tree = Arc::new(Tree::new(data, bodies));
        self.selected
            .take_if(|id| !data.components.contains_key(id));
        self.closed.retain(|id| data.components.contains_key(id));
        self.generation = self.generation.wrapping_add(1);
        self.bom_error = None;
    }
}
struct DraggedPart(String);

impl Shell {
    fn component_edit(&mut self, ctx: &egui::Context, edit: StructureEdit) {
        if self.pending.is_some() {
            return;
        }
        let expected = self.data.clone();
        let root = self.root.clone();
        let strict = self.strict_edits;
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut project = faber_wiring::open(&root, false, None)?;
                if project.data != expected {
                    return Err("The assembly changed. Try the edit again.".into());
                }
                match edit {
                    StructureEdit::Assign(id, component) => project.batch_operations(
                        vec![Operation::set_part_info(PartInfo {
                            id,
                            component: Some(component),
                            ..Default::default()
                        })],
                        strict,
                    )?,
                    StructureEdit::Configure(name) => {
                        faber_wiring::use_configuration(&root, &mut project, &name, strict)?
                    }
                }
                Ok((project.data, project.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn restore_component_isolation(&mut self) {
        let Some((component, _)) = &self.structure.isolated else {
            return;
        };
        if !self.data.components.contains_key(component) {
            self.end_isolate();
            return;
        }
        let shown: BTreeSet<_> = faber_core::component_parts(&self.data, component, true)
            .into_iter()
            .collect();
        self.hidden = self
            .data
            .parts
            .keys()
            .filter(|id| !shown.contains(*id))
            .cloned()
            .collect();
        self.mesh_dirty = true;
    }
    fn end_isolate(&mut self) {
        if let Some((_, mut previous)) = self.structure.isolated.take() {
            previous.retain(|id| self.data.parts.contains_key(id));
            self.hidden = previous;
            self.mesh_dirty = true;
        }
    }

    pub(super) fn component_tree(&mut self, ui: &mut egui::Ui) {
        let tree = Arc::clone(&self.structure.tree);
        let bodies = Arc::clone(&self.bodies);
        let selected = self.selection();
        if self.data.components.is_empty() {
            ui.label(RichText::new("BODIES").size(11.).color(MUTED));
            ui.add_space(10.);
            for b in bodies.iter() {
                self.body_row(ui, b, &selected);
            }
            return;
        }
        let heading = ui
            .label(RichText::new("Components").strong())
            .on_hover_text("Drop a part here to move it out of its component.");
        self.component_drop(ui, &heading, "");
        self.tree_nodes(ui, &tree, &bodies, &selected, "");
        ui.horizontal_wrapped(|ui| {
            if let Some(id) = self.structure.selected.clone()
                && self
                    .structure
                    .isolated
                    .as_ref()
                    .is_none_or(|(active, _)| active != &id)
                && ui.button("Isolate").clicked()
            {
                self.end_isolate();
                self.structure.isolated = Some((id, self.hidden.clone()));
                self.restore_component_isolation();
            }
            if self.structure.isolated.is_some() && ui.button("End isolate").clicked() {
                self.end_isolate();
            }
        });
    }
    fn tree_nodes(
        &mut self,
        ui: &mut egui::Ui,
        tree: &Tree,
        bodies: &[Body],
        selected: &[String],
        parent: &str,
    ) {
        for id in tree.children.get(parent).into_iter().flatten() {
            ui.push_id(("assembly", id), |ui| {
                let closed = self.structure.closed.contains(id);
                ui.horizontal(|ui| {
                    let state = if closed { "Expand" } else { "Collapse" };
                    let (_, arrow) = ui.allocate_exact_size(egui::vec2(18., 24.), Sense::click());
                    let label = format!("{state} {}", self.data.components[id].name);
                    arrow.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                    });
                    egui::collapsing_header::paint_default_icon(
                        ui,
                        if closed { 0. } else { 1. },
                        &arrow,
                    );
                    if arrow.on_hover_text(label).clicked() {
                        if closed {
                            self.structure.closed.remove(id);
                        } else {
                            self.structure.closed.insert(id.clone());
                        }
                    }
                    let r = ui.add(
                        egui::Button::selectable(
                            self.structure.selected.as_ref() == Some(id),
                            &self.data.components[id].name,
                        )
                        .truncate(),
                    );
                    if r.clicked() {
                        self.select_only(None);
                        self.structure.selected = Some(id.clone());
                        self.mesh_dirty = true;
                    }
                    self.component_drop(ui, &r, id);
                });
                if !self.structure.closed.contains(id) {
                    ui.indent("children", |ui| {
                        self.tree_nodes(ui, tree, bodies, selected, id)
                    });
                }
            });
        }
        for &n in tree.parts.get(parent).into_iter().flatten() {
            self.body_row(ui, &bodies[n], selected);
        }
    }
    fn component_drop(&mut self, ui: &egui::Ui, response: &egui::Response, component: &str) {
        if response.dnd_hover_payload::<DraggedPart>().is_some() {
            ui.painter().rect_stroke(
                response.rect,
                4.,
                egui::Stroke::new(1., ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(part) = response.dnd_release_payload::<DraggedPart>()
            && self
                .data
                .parts
                .get(&part.0)
                .is_some_and(|p| p.component.as_deref().unwrap_or_default() != component)
        {
            self.component_edit(
                ui.ctx(),
                StructureEdit::Assign(part.0.clone(), component.into()),
            );
        }
    }
    fn body_row(&mut self, ui: &mut egui::Ui, body: &Body, chosen: &[String]) {
        ui.horizontal(|ui| {
            let mut visible = !self.hidden.contains(&body.id);
            let toggle = ui.checkbox(&mut visible, "");
            ui.ctx().accesskit_node_builder(toggle.id, |node| {
                node.set_label(format!("Show {}", body.name));
            });
            if toggle.on_hover_text("Show body").changed() {
                if visible {
                    self.hidden.remove(&body.id);
                } else {
                    self.hidden.insert(body.id.clone());
                }
                self.mesh_dirty = true;
            }
            let selected = chosen.contains(&body.id);
            let row = ui
                .add(
                    egui::Button::selectable(selected, &body.name)
                        .truncate()
                        .sense(Sense::click_and_drag()),
                )
                .on_hover_text(&body.name);
            if !self.data.components.is_empty() {
                row.dnd_set_drag_payload(DraggedPart(body.id.clone()));
            }
            if row.clicked() || row.secondary_clicked() {
                self.structure.selected = None;
                let (ctrl, shift) =
                    ui.input(|i| (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift));
                if row.secondary_clicked() {
                    self.select_menu_target(Some(body.id.clone()));
                } else if shift {
                    self.select_range(&body.id);
                } else if row.clicked() && ctrl {
                    self.toggle_part(&body.id);
                } else if row.clicked() && chosen.len() == 1 && selected {
                    self.select_only(None);
                } else {
                    self.select_only(Some(body.id.clone()));
                }
                self.select_part_joint();
                self.mesh_dirty = true;
            }
            row.context_menu(|ui| self.body_menu(ui));
        });
    }

    pub(super) fn configuration_selector(&mut self, ui: &mut egui::Ui) {
        if self.data.configurations.is_empty() {
            ui.add_sized(
                [150., 29.],
                egui::Label::new(RichText::new(&self.data.name).color(MUTED)).truncate(),
            )
            .on_hover_text(&self.data.name);
            return;
        }
        let active = self.data.active_configuration.as_deref();
        let mut next = None;
        ui.allocate_ui_with_layout(
            egui::vec2(150., 29.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_max_width(150.);
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("assembly_configurations")
                        .width(150.)
                        .truncate()
                        .selected_text(active.unwrap_or("Configuration"))
                        .show_ui(ui, |ui| {
                            ui.set_max_width(245.);
                            for key in self.data.configurations.keys() {
                                if ui
                                    .add(
                                        egui::Button::selectable(active == Some(key.as_str()), key)
                                            .truncate(),
                                    )
                                    .on_hover_text(key)
                                    .clicked()
                                    && active != Some(key.as_str())
                                {
                                    next = Some(key.clone());
                                }
                            }
                        })
                        .response
                        .on_hover_text(active.unwrap_or("Configuration"));
                });
            },
        );
        if let Some(name) = next {
            self.component_edit(ui.ctx(), StructureEdit::Configure(name));
        }
    }
    pub(super) fn parts_list_panel(&mut self, ctx: &egui::Context) {
        if !self.structure.bom_open {
            return;
        }
        let current = self.structure.generation;
        if let Some((generation, job)) = &self.structure.bom_job {
            let result = job.try_recv();
            if !matches!(result, Err(mpsc::TryRecvError::Empty)) {
                if *generation == current {
                    match result {
                        Ok(rows) => {
                            self.structure.bom_rows = Some((current, rows));
                            self.structure.bom_error = None;
                        }
                        Err(_) => {
                            self.structure.bom_error = Some("The parts list worker stopped.");
                        }
                    }
                }
                self.structure.bom_job = None;
            }
        }
        if self
            .structure
            .bom_rows
            .as_ref()
            .is_none_or(|(generation, _)| *generation != current)
            && self.structure.bom_job.is_none()
            && self.structure.bom_error.is_none()
        {
            let data = self.data.clone();
            let bodies = Arc::clone(&self.bodies);
            let (tx, rx) = mpsc::channel();
            self.structure.bom_job = Some((current, rx));
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(faber_core::bom::bom(&data, &bodies, None, true));
                ctx.request_repaint();
            });
        }
        let area = ctx.content_rect();
        let width = (area.width() - 40.).clamp(220., 680.);
        egui::Window::new("Parts list")
            .id(egui::Id::unique("assembly_bom"))
            .default_pos(area.center() - egui::vec2(width / 2., 110.))
            .default_size([width, 200.])
            .min_width(220.)
            .resizable(true)
            .open(&mut self.structure.bom_open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(29.);
                    if let Some(e) = self.structure.bom_error {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                        if ui.button("Retry parts list").clicked() {
                            self.structure.bom_error = None;
                            self.structure.bom_rows = None;
                        }
                    } else if self
                        .structure
                        .bom_rows
                        .as_ref()
                        .is_some_and(|(generation, _)| *generation == current)
                    {
                        ui.small(format!("Project revision {}", self.data.revision));
                    } else {
                        ui.small("Updating parts list…");
                    }
                });
                let full_width =
                    COLUMNS.iter().map(|(_, w)| w).sum::<f32>() + ui.spacing().item_spacing.x * 5.;
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(full_width);
                        ui.horizontal(|ui| {
                            for &(title, width) in &COLUMNS {
                                cell(ui, width, RichText::new(title).strong(), title);
                            }
                        });
                        if let Some((_, rows)) = &self.structure.bom_rows {
                            for row in rows {
                                ui.horizontal(|ui| {
                                    let source = match &row.source {
                                        faber_core::bom::BomSource::Made => {
                                            ("Made", "Made".to_string())
                                        }
                                        faber_core::bom::BomSource::Purchased {
                                            designation,
                                            source,
                                            licence,
                                        } => (designation.as_str(), format!("{source}\n{licence}")),
                                    };
                                    let item = row.item.to_string();
                                    let quantity = row.quantity.to_string();
                                    let mass = row
                                        .mass_g
                                        .map_or_else(|| "—".into(), |g| format!("{g:.2}"));
                                    let material = row.material.as_deref().unwrap_or("—");
                                    let ids = format!("{}\n{}", row.name, row.parts.join(", "));
                                    for ((_, w), (text, tip)) in COLUMNS.iter().zip([
                                        (item.as_str(), item.as_str()),
                                        (quantity.as_str(), quantity.as_str()),
                                        (row.name.as_str(), ids.as_str()),
                                        (material, material),
                                        (mass.as_str(), mass.as_str()),
                                        (source.0, source.1.as_str()),
                                    ]) {
                                        cell(ui, *w, RichText::new(text), tip);
                                    }
                                });
                            }
                        }
                    });
            });
    }
}
const COLUMNS: [(&str, f32); 6] = [
    ("Item", 42.),
    ("Quantity", 64.),
    ("Name", 168.),
    ("Material", 105.),
    ("Mass (g)", 72.),
    ("Source", 110.),
];
fn cell(ui: &mut egui::Ui, width: f32, text: RichText, tip: &str) {
    ui.add_sized(
        [width, 22.],
        egui::Label::new(text).halign(egui::Align::Min).truncate(),
    )
    .on_hover_text(tip);
}
enum StructureEdit {
    Assign(String, String),
    Configure(String),
}

#[cfg(test)]
impl Browser {
    pub(super) fn hold_bom(&mut self) -> mpsc::Sender<Vec<BomRow>> {
        let (tx, rx) = mpsc::channel();
        self.bom_job = Some((self.generation, rx));
        tx
    }
}
