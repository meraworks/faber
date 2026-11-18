use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolWindow {
    CreateProject,
    MoveRotate,
    Import,
    Export,
    Checks,
    Parameters,
    Feature,
    Sketch,
    MoveCopy,
}

impl ToolWindow {
    fn title(self) -> &'static str {
        match self {
            Self::CreateProject => "Create project",
            Self::MoveRotate => "Move / rotate part",
            Self::Import => "Import model",
            Self::Export => "Export design",
            Self::Checks => "Design checks",
            Self::Parameters => "Edit parameters",
            Self::Feature => "Feature",
            Self::Sketch => "Create sketch",
            Self::MoveCopy => "MOVE/COPY",
        }
    }
}

pub(super) enum Draft {
    Project(ProjectDraft),
    Placement(PlacementDraft),
    Import(ImportDraft),
    Export,
    Checks,
    Parameters(ProjectData, BTreeMap<String, String>),
    Feature(crate::feature_form::FeatureForm),
    Sketch(crate::sketch::SketchDraft),
    Move(Box<move_tool::MoveDraft>),
}

impl Draft {
    fn kind(&self) -> ToolWindow {
        match self {
            Self::Project(_) => ToolWindow::CreateProject,
            Self::Placement(_) => ToolWindow::MoveRotate,
            Self::Import(_) => ToolWindow::Import,
            Self::Export => ToolWindow::Export,
            Self::Checks => ToolWindow::Checks,
            Self::Parameters(..) => ToolWindow::Parameters,
            Self::Feature(_) => ToolWindow::Feature,
            Self::Sketch(_) => ToolWindow::Sketch,
            Self::Move(_) => ToolWindow::MoveCopy,
        }
    }
}

#[derive(Default)]
pub(crate) struct WindowState {
    pub open: Option<ToolWindow>,
    pub error: Option<String>,
    pub dirty: bool,
    pub(super) stale: bool,
    pub(super) saving: bool,
    pub(super) running: bool,
    pub(super) next: Option<Box<Draft>>,
    pub(super) apply: bool,
}

impl WindowState {
    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }
    pub(super) fn finish(&mut self) {
        self.cancel();
    }
    fn open(&mut self, kind: ToolWindow) {
        self.finish();
        self.open = Some(kind);
    }
    pub(super) fn reload(&mut self) {
        self.next = None;
        if self.dirty {
            self.stale = true;
            self.error = Some(
                "The project changed on disk. Apply is disabled until you reopen the form.".into(),
            );
        }
    }
}

impl Shell {
    pub(super) fn cancel_window(&mut self) {
        if self.error == self.window.error {
            self.error = None;
        }
        if self.move_draft.is_some() {
            self.cancel_move();
        }
        if self.project_draft.is_some() {
            self.dialog = None;
        }
        self.project_draft = None;
        self.feature_draft = None;
        self.new_parameter = Default::default();
        self.parameter_draft = None;
        self.placement_draft = None;
        self.sketch_draft = None;
        self.import_draft = None;
        if self.export_open {
            self.export_path.clear();
            self.export_force = false;
            self.export_format = "step".into();
        }
        self.export_open = false;
        self.checks_open = false;
        self.window.cancel();
    }

    pub(super) fn request_window(&mut self, draft: Draft) {
        if self.window.dirty {
            self.window.next = Some(Box::new(draft));
            return;
        }
        self.cancel_window();
        self.window.open(draft.kind());
        match draft {
            Draft::Project(d) => self.project_draft = Some(d),
            Draft::Placement(d) => self.placement_draft = Some(d),
            Draft::Import(d) => self.import_draft = Some(d),
            Draft::Export => {
                self.export_path = self.root.join("out").to_string_lossy().into_owned();
                self.export_open = true;
            }
            Draft::Checks => self.checks_open = true,
            Draft::Parameters(data, values) => self.parameter_draft = Some((data, values)),
            Draft::Feature(d) => self.feature_draft = Some(d),
            Draft::Sketch(d) => self.sketch_draft = Some(d),
            Draft::Move(d) => self.move_draft = Some(*d),
        }
    }

    pub(super) fn discard_question(&mut self, ctx: &egui::Context) {
        if self.window.next.is_none() {
            return;
        }
        let title = match self.feature_draft.as_ref() {
            Some(form) if form.create => "New feature",
            Some(_) => "Edit feature",
            None => self.window.open.map_or("the form", ToolWindow::title),
        };
        let mut discard = false;
        let mut keep = false;
        egui::Modal::new(egui::Id::unique("discard_changes")).show(ctx, |ui| {
            ui.label(format!("Discard the changes in {title}?"));
            ui.horizontal(|ui| {
                discard = ui.button("Discard").clicked();
                keep = ui.button("Keep").clicked();
            });
        });
        if keep {
            self.window.next = None;
        }
        if discard && let Some(next) = self.window.next.take() {
            self.window.dirty = false;
            self.request_window(*next);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowEvent {
    None,
    Apply,
    Cancel,
}

pub(super) fn tool_window(
    ctx: &egui::Context,
    state: &mut WindowState,
    kind: ToolWindow,
    window: egui::Window<'_>,
    bounds: egui::Rect,
    body: impl FnOnce(&mut egui::Ui, &mut Option<String>, bool),
) -> (WindowEvent, Option<egui::Rect>) {
    state.open.get_or_insert(kind);
    let mut open = true;
    let mut edited = false;
    let height = (bounds.height() - 68.).max(40.);
    let shown = window
        .open(&mut open)
        .default_height(height)
        .constrain_to(bounds)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tool_body")
                .max_height(height)
                .show(ui, |ui| {
                    let first = ui.output(|output| output.events.len());
                    body(ui, &mut state.error, !state.stale);
                    edited = ui.output(|output| {
                        output.events[first..].iter().any(|event| {
                            let info = event.widget_info();
                            info.prev_text_value.is_some()
                                || matches!(event, egui::output::OutputEvent::Clicked(_))
                                    && (info.role == egui::Role::CheckBox
                                        || info.selected == Some(false))
                        })
                    });
                    if let Some(error) = &state.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                });
        });
    state.dirty |= edited;
    let event = if !open {
        WindowEvent::Cancel
    } else if !state.stale && std::mem::take(&mut state.apply) {
        WindowEvent::Apply
    } else {
        WindowEvent::None
    };
    (event, shown.map(|s| s.response.rect))
}
