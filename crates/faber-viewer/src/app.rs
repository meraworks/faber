use crate::mark;
use eframe::{egui, egui_wgpu};
use egui::{Color32, RichText, Sense, Vec2};
use faber_model::ProjectData;
use faber_ports::Body;
use faber_render::{DrawList, Gpu, Target};
use faber_scene::{Camera, Section};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

const PANEL: Color32 = Color32::from_rgb(19, 25, 33);
const MUTED: Color32 = Color32::from_rgb(132, 148, 166);
const ACCENT: Color32 = Color32::from_rgb(100, 205, 192);

fn section_normal(azimuth: f64, tilt: f64) -> [f64; 3] {
    let (azimuth, tilt) = (azimuth.to_radians(), tilt.to_radians());
    [
        tilt.cos() * azimuth.cos(),
        tilt.cos() * azimuth.sin(),
        tilt.sin(),
    ]
}

type Loaded = Result<(ProjectData, Vec<Body>), String>;
type Exported = Result<(u64, Vec<String>), String>;

struct ImportDraft {
    expected: ProjectData,
    path: String,
    name: String,
}

struct PlacementDraft {
    expected: ProjectData,
    part: String,
    translation: [String; 3],
    rotation: [String; 3],
}

struct ProjectDraft {
    create: bool,
    path: String,
    name: String,
}

enum ToolKind {
    Plane,
    FacePlane,
    Axis,
    Point,
    Linear,
    Circular,
    Mirror,
    Shell,
    Fillet,
    Chamfer,
    Offset,
    Draft,
    Thicken,
    Hole,
    Split,
}

pub struct Shell {
    project_loaded: bool,
    root: PathBuf,
    pub data: ProjectData,
    pub bodies: Vec<Body>,
    pub camera: Camera,
    pub section_on: bool,
    pub plane: Section,
    pub selected: Option<String>,
    pub section_selected: Option<String>,
    section_pending: Option<mpsc::Receiver<(String, Result<Section, String>)>>,
    pub hidden: BTreeSet<String>,
    gpu: Gpu,
    target: Option<Target>,
    texture: Option<egui::TextureId>,
    list: Option<DrawList>,
    dirty: bool,
    mesh_dirty: bool,
    pub error: Option<String>,
    last_poll: Instant,
    modified: Option<SystemTime>,
    pending: Option<mpsc::Receiver<Loaded>>,
    frame_ms: f64,
    parameter_draft: Option<(ProjectData, BTreeMap<String, String>)>,
    saving: bool,
    feature_draft: Option<crate::feature_form::FeatureForm>,
    sketch_draft: Option<crate::sketch::SketchDraft>,
    import_draft: Option<ImportDraft>,
    placement_draft: Option<PlacementDraft>,
    project_draft: Option<ProjectDraft>,
    pending_project: Option<PathBuf>,
    check_pending: Option<
        mpsc::Receiver<(
            ProjectData,
            Result<faber_wiring::InterferenceReport, String>,
        )>,
    >,
    check_result: Option<(ProjectData, faber_wiring::InterferenceReport)>,
    checks_open: bool,
    strict_edits: bool,
    export_open: bool,
    export_path: String,
    export_format: String,
    export_force: bool,
    export_pending: Option<mpsc::Receiver<Exported>>,
    export_message: Option<String>,
}

impl Shell {
    pub fn welcome(cc: &eframe::CreationContext<'_>) -> Self {
        let data = ProjectData {
            schema: 1,
            revision: 0,
            name: String::new(),
            parts: Default::default(),
            params: Default::default(),
            contacts: Default::default(),
            sections: Default::default(),
        };
        let mut shell = Self::new(cc, PathBuf::new(), data, vec![], None);
        shell.project_loaded = false;
        shell
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: PathBuf,
        data: ProjectData,
        bodies: Vec<Body>,
        plane: Option<Section>,
    ) -> Self {
        style(&cc.egui_ctx);
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("The viewer needs the GPU renderer");
        let camera = Camera::fit(&bodies);
        let modified = std::fs::metadata(root.join("faber.json"))
            .and_then(|m| m.modified())
            .ok();
        Self {
            project_loaded: true,
            root,
            data,
            bodies,
            camera,
            section_on: plane.is_some(),
            plane: plane.unwrap_or(Section {
                normal: [1., 0., 0.],
                offset: 0.,
                reverse: false,
            }),
            selected: None,
            section_selected: None,
            section_pending: None,
            hidden: BTreeSet::new(),
            gpu: Gpu::new(rs.device.clone(), rs.queue.clone()),
            target: None,
            texture: None,
            list: None,
            dirty: true,
            mesh_dirty: true,
            error: None,
            last_poll: Instant::now(),
            modified,
            pending: None,
            frame_ms: 0.,
            parameter_draft: None,
            saving: false,
            feature_draft: None,
            sketch_draft: None,
            import_draft: None,
            placement_draft: None,
            project_draft: None,
            pending_project: None,
            check_pending: None,
            check_result: None,
            checks_open: false,
            strict_edits: false,
            export_open: false,
            export_path: String::new(),
            export_format: "step".into(),
            export_force: false,
            export_pending: None,
            export_message: None,
        }
    }

    fn fit(&mut self) {
        self.camera = Camera::fit(&self.bodies);
        self.dirty = true;
    }

    fn view_controls(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(self.section_on, "Section").clicked() {
                self.section_on = !self.section_on;
                self.mesh_dirty = true;
            }
            ui.add_space(10.);
            if ui.button("Fit").clicked() {
                self.fit();
            }
            if ui.button("Top").clicked() {
                self.camera.set_view(faber_scene::View::Top);
                self.dirty = true;
            }
            if ui.button("Front").clicked() {
                self.camera.set_view(faber_scene::View::Front);
                self.dirty = true;
            }
            if ui.button("Iso").clicked() {
                self.fit();
            }
        });
    }

    fn project_panel(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.project_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if draft.create {
            "Create project"
        } else {
            "Choose project"
        })
        .default_width(460.)
        .show(ctx, |ui| {
            ui.label(
                "Use an absolute folder path. Saved changes in the current project remain on disk.",
            );
            ui.add_enabled_ui(!self.saving, |ui| {
                let label = ui.label("Project folder");
                ui.text_edit_singleline(&mut draft.path)
                    .labelled_by(label.id);
                if draft.create {
                    let label = ui.label("Project name");
                    ui.text_edit_singleline(&mut draft.name)
                        .labelled_by(label.id);
                }
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(
                            self.pending.is_none() && !draft.path.trim().is_empty(),
                            egui::Button::new("Load project"),
                        )
                        .clicked();
                    cancel = ui.button("Cancel project").clicked();
                });
            });
            if self.saving {
                ui.spinner();
            }
            if let Some(error) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
        if apply {
            let path = PathBuf::from(draft.path.trim());
            if !path.is_absolute() {
                self.error = Some("Use an absolute project folder path".into());
            } else {
                let name = draft.create.then(|| draft.name.trim().to_owned());
                let ctx = ctx.clone();
                let (tx, rx) = mpsc::channel();
                self.pending = Some(rx);
                self.pending_project = Some(path.clone());
                self.saving = true;
                self.error = None;
                std::thread::spawn(move || {
                    let _ = tx.send(crate::edit::load_project(&path, name));
                    ctx.request_repaint();
                });
            }
        }
        if cancel {
            self.project_draft = None;
        }
    }

    fn placement_panel(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.placement_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Move / rotate part").default_width(380.).show(ctx, |ui| {
            ui.heading(&draft.part);
            ui.label("Position uses mm. Rotation uses degrees, X then Y then Z, about the part origin.");
            ui.label("Values can use project parameters.");
            ui.add_enabled_ui(!self.saving, |ui| {
                egui::Grid::new("placement_fields").show(ui, |ui| {
                    for (prefix, values) in [("Position", &mut draft.translation), ("Rotation", &mut draft.rotation)] {
                        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                            let label = ui.label(format!("{prefix} {axis}"));
                            ui.text_edit_singleline(value).labelled_by(label.id);
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    apply = ui.add_enabled(self.pending.is_none(), egui::Button::new("Apply placement")).clicked();
                    cancel = ui.button("Cancel placement").clicked();
                });
            });
            if self.saving { ui.spinner(); }
            if let Some(error) = &self.error { ui.colored_label(Color32::LIGHT_RED, error); }
        });
        if apply {
            let root = self.root.clone();
            let expected = draft.expected.clone();
            let part = draft.part.clone();
            let placement = faber_model::Placement {
                translation: draft
                    .translation
                    .clone()
                    .map(faber_model::Scalar::Expression),
                rotation: draft.rotation.clone().map(faber_model::Scalar::Expression),
            };
            let strict = self.strict_edits;
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.saving = true;
            self.error = None;
            std::thread::spawn(move || {
                let _ = tx.send(crate::edit::place(
                    &root, &expected, &part, placement, strict,
                ));
                ctx.request_repaint();
            });
        }
        if cancel {
            self.placement_draft = None;
        }
    }

    fn import_panel(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.import_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Import STEP solid")
            .default_width(460.)
            .show(ctx, |ui| {
                ui.label("Import a model you own. The project keeps a copy of the source file.");
                ui.add_enabled_ui(!self.saving, |ui| {
                    let label = ui.label("Source file");
                    ui.text_edit_singleline(&mut draft.path)
                        .labelled_by(label.id);
                    let label = ui.label("Part name");
                    ui.text_edit_singleline(&mut draft.name)
                        .labelled_by(label.id);
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                self.pending.is_none()
                                    && !draft.path.trim().is_empty()
                                    && !draft.name.trim().is_empty(),
                                egui::Button::new("Import solid"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel import").clicked();
                    });
                });
                if self.saving {
                    ui.spinner();
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        if apply {
            let root = self.root.clone();
            let source = PathBuf::from(draft.path.trim());
            let source = if source.is_absolute() {
                source
            } else {
                root.join(source)
            };
            let expected = draft.expected.clone();
            let name = draft.name.trim().to_owned();
            let strict = self.strict_edits;
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.saving = true;
            self.error = None;
            std::thread::spawn(move || {
                let _ = tx.send(crate::edit::import(&root, &expected, &source, name, strict));
                ctx.request_repaint();
            });
        }
        if cancel {
            self.import_draft = None;
        }
    }

    fn export_panel(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new("Export design").open(&mut open).default_width(500.).show(ctx, |ui| {
            ui.add_enabled_ui(self.export_pending.is_none(), |ui| {
                let label = ui.label("Output directory");
                ui.text_edit_singleline(&mut self.export_path).labelled_by(label.id);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.export_format,"step".into(),"STEP solids");
                    ui.selectable_value(&mut self.export_format,"stl".into(),"STL meshes");
                });
                ui.checkbox(&mut self.export_force,"Export despite check findings");
                ui.label(format!("Revision {} · {} bodies · mm",self.data.revision,self.bodies.len()));
                ui.label("Exports include hidden bodies and the full solids before section analysis.");
                run = ui.add_enabled(self.pending.is_none() && !self.export_path.trim().is_empty(),egui::Button::new("Write files")).clicked();
            });
            if self.export_pending.is_some() { ui.spinner(); ui.label("Checking and writing files…"); }
            if let Some(message) = &self.export_message { ui.label(message); }
        });
        self.export_open = open;
        if run {
            let path = PathBuf::from(&self.export_path);
            let path = if path.is_absolute() {
                path
            } else {
                self.root.join(path)
            };
            let format = self.export_format.clone();
            let force = self.export_force;
            let bodies = self.bodies.clone();
            let contacts = self.data.contacts.clone();
            let revision = self.data.revision;
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.export_pending = Some(rx);
            self.export_message = None;
            std::thread::spawn(move || {
                let result =
                    faber_wiring::export_checked(&bodies, &contacts, &path, &format, force)
                        .map(|files| (revision, files));
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }

    fn checks(&mut self, ctx: &egui::Context) {
        if !self.checks_open {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new("Design checks")
            .open(&mut open)
            .default_width(480.)
            .show(ctx, |ui| {
                ui.label("Mesh interference and clearance below 0.1 mm. Depth is approximate.");
                ui.checkbox(&mut self.strict_edits, "Refuse edits that add check errors");
                run = ui
                    .add_enabled(
                        self.check_pending.is_none() && self.pending.is_none(),
                        egui::Button::new("Run interference check"),
                    )
                    .clicked();
                if self.check_pending.is_some() {
                    ui.spinner();
                }
                if let Some((data, report)) = &self.check_result {
                    if data != &self.data {
                        ui.colored_label(Color32::YELLOW, "Design changed. Run the check again.");
                    } else {
                        ui.label(format!(
                            "Revision {} · {} findings",
                            data.revision,
                            report.violations.len()
                        ));
                        if report.violations.is_empty() {
                            ui.label("No mesh interference or clearance findings.");
                        }
                        egui::ScrollArea::vertical()
                            .max_height(320.)
                            .show(ui, |ui| {
                                for finding in &report.violations {
                                    ui.group(|ui| {
                                        ui.label(format!("{} / {}", finding.a, finding.b));
                                        if finding.surface_crossing && finding.depth_mm <= 1e-9 {
                                            ui.label("Surface crossing; depth is not resolved.");
                                        } else {
                                            ui.label(format!(
                                                "Estimated depth: {:.3} mm · Gap: {:.3} mm",
                                                finding.depth_mm, finding.gap_mm
                                            ));
                                        }
                                        ui.horizontal(|ui| {
                                            for id in [&finding.a, &finding.b] {
                                                if ui.button(format!("Select {id}")).clicked() {
                                                    self.selected = Some(id.clone());
                                                    self.mesh_dirty = true;
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                    }
                }
            });
        self.checks_open = open;
        if run {
            let bodies = self.bodies.clone();
            let data = self.data.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.check_pending = Some(rx);
            std::thread::spawn(move || {
                let result = faber_wiring::interference(&bodies, 0.1, &data.contacts);
                let _ = tx.send((data, result));
                ctx.request_repaint();
            });
        }
    }

    fn move_history(&mut self, ctx: &egui::Context, redo: bool) {
        if self.pending.is_some()
            || self.parameter_draft.is_some()
            || self.feature_draft.is_some()
            || self.sketch_draft.is_some()
            || self.import_draft.is_some()
            || self.placement_draft.is_some()
            || self.project_draft.is_some()
        {
            return;
        }
        let root = self.root.clone();
        let expected = self.data.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        std::thread::spawn(move || {
            let _ = tx.send(crate::edit::history(&root, &expected, redo));
            ctx.request_repaint();
        });
    }

    fn parameter_editor(&mut self, ctx: &egui::Context) {
        let Some((_, values)) = &mut self.parameter_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Edit parameters")
            .collapsible(false)
            .resizable(true)
            .default_width(420.)
            .show(ctx, |ui| {
                ui.label("Enter a value or an expression. Lengths use mm.");
                ui.add_enabled_ui(!self.saving, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.)
                        .show(ui, |ui| {
                            egui::Grid::new("parameter_editor").show(ui, |ui| {
                                for (name, expression) in values {
                                    let label = ui.label(name);
                                    ui.text_edit_singleline(expression).labelled_by(label.id);
                                    ui.end_row();
                                }
                            });
                        });
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                self.pending.is_none(),
                                egui::Button::new("Apply parameters"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if self.saving {
                    ui.spinner();
                    ui.label("Building model…");
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        if cancel {
            self.parameter_draft = None;
        }
        if apply && let Some((expected, values)) = self.parameter_draft.clone() {
            let root = self.root.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.saving = true;
            self.error = None;
            let strict = self.strict_edits;
            std::thread::spawn(move || {
                let result = if strict {
                    crate::edit::strict_edit(
                        &root,
                        &expected,
                        values
                            .into_iter()
                            .map(|(name, expression)| faber_model::Edit::SetParam {
                                name,
                                expression,
                            })
                            .collect(),
                    )
                } else {
                    crate::edit::parameters(&root, &expected, values)
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }

    fn new_feature(&mut self, part_id: &str, kind: ToolKind) -> Result<(), String> {
        use faber_model::{Geometry, Scalar};
        let part = self.data.parts.get(part_id).ok_or("Select a part")?;
        let prefix = match kind {
            ToolKind::Plane => "plane",
            ToolKind::FacePlane => "face_plane",
            ToolKind::Axis => "axis",
            ToolKind::Point => "point",
            ToolKind::Shell => "shell",
            ToolKind::Mirror => "mirror",
            ToolKind::Fillet => "fillet",
            ToolKind::Chamfer => "chamfer",
            ToolKind::Offset => "offset",
            ToolKind::Draft => "draft",
            ToolKind::Thicken => "thicken",
            ToolKind::Hole => "hole",
            ToolKind::Split => "split",
            _ => "pattern",
        };
        let (source, id) = match &part.geometry {
            Geometry::History { features, output } => {
                let id = (1..)
                    .map(|n| format!("{prefix}_{n}"))
                    .find(|id| {
                        !features
                            .iter()
                            .any(|feature| feature.id.eq_ignore_ascii_case(id))
                    })
                    .ok_or("No feature id is available")?;
                (output.clone(), id)
            }
            _ => ("source".into(), format!("{prefix}_1")),
        };
        let body = self
            .bodies
            .iter()
            .find(|body| body.id == part_id)
            .ok_or("Part has no solid")?;
        let pitch = ((body.bounds[1][0] - body.bounds[0][0]) * 1.5).max(1.);
        let geometry = if matches!(kind, ToolKind::FacePlane) {
            Geometry::PlaneOnFace {
                source,
                face: faber_model::FaceSelector {
                    created_by: None,
                    normal: Some([0_f64.into(), 0_f64.into(), 1_f64.into()]),
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: 0.000001_f64.into(),
                    expect: 1,
                },
                offset: 0_f64.into(),
            }
        } else if matches!(kind, ToolKind::Plane) {
            Geometry::Plane {
                placement: Default::default(),
            }
        } else if matches!(kind, ToolKind::Axis) {
            Geometry::Axis {
                origin: [0_f64.into(), 0_f64.into(), 0_f64.into()],
                direction: [0_f64.into(), 0_f64.into(), 1_f64.into()],
            }
        } else if matches!(kind, ToolKind::Point) {
            Geometry::Point {
                position: [0_f64.into(), 0_f64.into(), 0_f64.into()],
            }
        } else if matches!(kind, ToolKind::Split) {
            Geometry::Split {
                source,
                origin: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(0.)],
                normal: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)],
                keep: faber_model::SplitKeep::Both,
            }
        } else if matches!(kind, ToolKind::Hole) {
            Geometry::Hole {
                source,
                diameter: Scalar::Number(4.),
                depth: Scalar::Number(10.),
                placement: Default::default(),
                head: None,
            }
        } else if matches!(kind, ToolKind::Thicken) {
            Geometry::Thicken {
                source,
                thickness: Scalar::Number(1.),
                face: Some(faber_model::FaceSelector {
                    created_by: None,
                    normal: Some([Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)]),
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: Scalar::Number(0.000001),
                    expect: 1,
                }),
            }
        } else if matches!(kind, ToolKind::Draft) {
            Geometry::Draft {
                source,
                faces: faber_model::FaceSelector {
                    created_by: None,
                    normal: Some([Scalar::Number(1.), Scalar::Number(0.), Scalar::Number(0.)]),
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: Scalar::Number(0.000001),
                    expect: 1,
                },
                direction: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)],
                origin: std::array::from_fn(|_| Scalar::Number(0.)),
                normal: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)],
                angle: Scalar::Number(5.),
            }
        } else if matches!(kind, ToolKind::Offset) {
            Geometry::Offset {
                source,
                distance: Scalar::Number(1.),
            }
        } else if matches!(kind, ToolKind::Fillet | ToolKind::Chamfer) {
            let edges = faber_model::EdgeSelector {
                between: None,
                of_face: None,
                parallel: Some([Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)]),
                near_centre: None,
                in_box: None,
                length: None,
                radius: None,
                tolerance: Scalar::Number(0.000001),
                expect: 4,
            };
            if matches!(kind, ToolKind::Fillet) {
                Geometry::Fillet {
                    source,
                    edges,
                    radius: Scalar::Number(1.),
                }
            } else {
                Geometry::Chamfer {
                    source,
                    edges,
                    distance: Scalar::Number(1.),
                }
            }
        } else if matches!(kind, ToolKind::Shell) {
            Geometry::Shell {
                source,
                faces: faber_model::FaceSelector {
                    created_by: None,
                    normal: Some([Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)]),
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: Scalar::Number(0.000001),
                    expect: 1,
                },
                thickness: Scalar::Number(1.),
                thickness_overrides: Vec::new(),
            }
        } else if matches!(kind, ToolKind::Mirror) {
            Geometry::Mirror {
                source,
                origin: std::array::from_fn(|_| Scalar::Number(0.)),
                normal: [Scalar::Number(1.), Scalar::Number(0.), Scalar::Number(0.)],
                keep_source: true,
            }
        } else if matches!(kind, ToolKind::Circular) {
            Geometry::CircularPattern {
                source,
                count: Scalar::Number(2.),
                origin: std::array::from_fn(|_| Scalar::Number(0.)),
                axis: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)],
                angle: Scalar::Number(180.),
            }
        } else {
            Geometry::LinearPattern {
                source,
                count: Scalar::Number(2.),
                step: [
                    Scalar::Number(pitch),
                    Scalar::Number(0.),
                    Scalar::Number(0.),
                ],
            }
        };
        let mut form = crate::feature_form::FeatureForm::new(
            self.data.clone(),
            part_id.into(),
            id,
            &geometry,
        )?;
        form.create = true;
        self.feature_draft = Some(form);
        self.error = None;
        Ok(())
    }

    fn feature_editor(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.feature_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if form.create {
            "New feature"
        } else {
            "Edit feature"
        })
        .default_width(460.)
        .show(ctx, |ui| {
            ui.heading(&form.id);
            ui.label("Lengths use mm. Angles use degrees.");
            if !form.help.is_empty() {
                ui.label(form.help);
            }
            ui.add_enabled_ui(!self.saving, |ui| {
                if let Err(error) = form.hole_options(ui) {
                    self.error = Some(error);
                }
                form.split_options(ui);
                egui::ScrollArea::vertical()
                    .max_height(420.)
                    .show(ui, |ui| {
                        egui::Grid::new("feature_fields").show(ui, |ui| {
                            for (path, enabled) in &mut form.flags {
                                ui.checkbox(enabled, crate::feature_form::label(path));
                                ui.end_row();
                            }
                            for (path, text, _) in &mut form.fields {
                                let label = ui.label(crate::feature_form::label(path));
                                ui.text_edit_singleline(text).labelled_by(label.id);
                                ui.end_row();
                            }
                        });
                    });
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(self.pending.is_none(), egui::Button::new("Apply feature"))
                        .clicked();
                    cancel = ui.button("Cancel feature").clicked();
                });
            });
            if self.saving {
                ui.spinner();
            }
            if let Some(error) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
        if apply {
            match form.geometry() {
                Err(error) => self.error = Some(error),
                Ok(geometry) => {
                    let root = self.root.clone();
                    let expected = form.expected.clone();
                    let part = form.part.clone();
                    let id = form.id.clone();
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.pending = Some(rx);
                    self.saving = true;
                    self.error = None;
                    let strict = self.strict_edits;
                    let create = form.create;
                    std::thread::spawn(move || {
                        let _ = tx.send(if create {
                            crate::edit::append_feature(
                                &root, &expected, &part, &id, geometry, strict,
                            )
                        } else if strict {
                            crate::edit::strict_edit(
                                &root,
                                &expected,
                                vec![faber_model::Edit::EditFeature { part, id, geometry }],
                            )
                        } else {
                            crate::edit::feature(&root, &expected, &part, &id, geometry)
                        });
                        ctx.request_repaint();
                    });
                }
            }
        }
        if cancel {
            self.feature_draft = None;
        }
    }

    fn sketch_editor(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.sketch_draft else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        egui::Window::new("Create sketch")
            .default_width(640.)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.saving, |ui| {
                    create = draft.ui(ui);
                    cancel = ui.button("Cancel sketch").clicked();
                });
                if self.saving {
                    ui.spinner();
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        if create && self.pending.is_none() {
            match draft.part() {
                Err(error) => self.error = Some(error),
                Ok(part) => {
                    let root = self.root.clone();
                    let expected = draft.expected.clone();
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.pending = Some(rx);
                    self.saving = true;
                    self.error = None;
                    let strict = self.strict_edits;
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::edit::create(&root, &expected, part, strict));
                        ctx.request_repaint();
                    });
                }
            }
        }
        if cancel {
            self.sketch_draft = None;
        }
    }

    /// Applies the selected visible analysis to the view. Drops a missing selection.
    /// The background thread resolves sections that need the model.
    fn sync_section(&mut self, ctx: &egui::Context) {
        let Some(id) = self.section_selected.clone() else {
            return;
        };
        self.mesh_dirty = true;
        let Some(section) = self.data.sections.get(&id) else {
            self.section_selected = None;
            self.section_on = false;
            return;
        };
        if !section.visible {
            self.section_on = false;
            return;
        }
        if let Some(plane) = crate::analysis::plane(section) {
            self.plane = plane;
            self.section_on = true;
            return;
        }
        self.section_on = false;
        let root = self.root.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.section_pending = Some(rx);
        std::thread::spawn(move || {
            let result = faber_wiring::open(&root, false, None)
                .and_then(|project| project.section(&id))
                .and_then(|resolved| {
                    crate::analysis::from_world(resolved.origin, resolved.normal, resolved.reverse)
                        .ok_or_else(|| "Section normal is zero".to_string())
                });
            let _ = tx.send((id, result));
            ctx.request_repaint();
        });
    }

    /// Applies a resolved analysis when it is still selected.
    fn apply_resolved_section(&mut self, id: String, result: Result<Section, String>) {
        if self.section_selected.as_deref() != Some(id.as_str()) {
            return;
        }
        match result {
            Ok(plane) => {
                let visible = self.data.sections.get(&id).is_some_and(|s| s.visible);
                self.plane = plane;
                self.section_on = visible;
                self.mesh_dirty = true;
            }
            Err(error) => {
                self.section_selected = None;
                self.section_on = false;
                self.error = Some(error);
            }
        }
    }

    /// Saves section changes in a background thread.
    fn save_sections(&mut self, ctx: &egui::Context, operations: Vec<faber_model::Edit>) {
        if self.pending.is_some() {
            return;
        }
        let root = self.root.clone();
        let expected = self.data.clone();
        let strict = self.strict_edits;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        std::thread::spawn(move || {
            let _ = tx.send(crate::edit::sections(&root, &expected, operations, strict));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.export_pending
            && let Ok(result) = rx.try_recv()
        {
            self.export_message = Some(match result {
                Ok((revision, files)) => {
                    format!("Exported {} files from revision {revision}.", files.len())
                }
                Err(error) => error,
            });
            self.export_pending = None;
        }
        if let Some(rx) = &self.check_pending
            && let Ok((data, result)) = rx.try_recv()
        {
            match result {
                Ok(report) => self.check_result = Some((data, report)),
                Err(error) => self.error = Some(error),
            }
            self.check_pending = None;
        }
        if let Some(rx) = &self.section_pending
            && let Ok((id, result)) = rx.try_recv()
        {
            self.section_pending = None;
            self.apply_resolved_section(id, result);
        }
        if let Some(rx) = &self.pending
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok((data, bodies)) => {
                    if let Some(root) = self.pending_project.take() {
                        self.project_loaded = true;
                        self.root = root;
                        self.modified = std::fs::metadata(self.root.join("faber.json"))
                            .and_then(|m| m.modified())
                            .ok();
                        self.camera = Camera::fit(&bodies);
                        self.dirty = true;
                        self.selected = None;
                        self.hidden.clear();
                        self.section_on = false;
                        self.section_selected = None;
                        self.section_pending = None;
                        self.check_result = None;
                        self.check_pending = None;
                        self.checks_open = false;
                        self.export_open = false;
                        self.export_path.clear();
                        self.export_message = None;
                        self.export_pending = None;
                    }
                    if self.saving {
                        self.parameter_draft = None;
                        self.feature_draft = None;
                        self.sketch_draft = None;
                        self.import_draft = None;
                        self.placement_draft = None;
                        self.project_draft = None;
                    }
                    self.data = data;
                    self.bodies = bodies;
                    self.mesh_dirty = true;
                    self.error = None;
                    self.sync_section(ctx);
                    self.hidden.retain(|id| self.data.parts.contains_key(id));
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|id| !self.data.parts.contains_key(id))
                    {
                        self.selected = None;
                    }
                }
                Err(e) => self.error = Some(e),
            }
            self.pending = None;
            self.pending_project = None;
            self.saving = false;
        }
        if self.project_loaded
            && self.pending.is_none()
            && self.last_poll.elapsed() > Duration::from_millis(500)
        {
            self.last_poll = Instant::now();
            let modified = std::fs::metadata(self.root.join("faber.json"))
                .and_then(|m| m.modified())
                .ok();
            if modified != self.modified {
                self.modified = modified;
                let root = self.root.clone();
                let ctx = ctx.clone();
                let (tx, rx) = mpsc::channel();
                self.pending = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(faber_wiring::read(&root));
                    ctx.request_repaint();
                });
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    /// Selects a saved section. Selecting the same section again clears the selection.
    fn select_section(&mut self, ctx: &egui::Context, id: String) {
        if self.section_selected.as_deref() == Some(id.as_str()) {
            self.section_selected = None;
            self.section_on = false;
        } else {
            self.section_selected = Some(id);
            self.sync_section(ctx);
        }
        self.mesh_dirty = true;
    }

    /// Lists the saved section analyses with show, select, flip and delete.
    fn analysis_list(&mut self, ui: &mut egui::Ui) {
        let idle = self.pending.is_none()
            && self.parameter_draft.is_none()
            && self.feature_draft.is_none()
            && self.sketch_draft.is_none()
            && self.import_draft.is_none()
            && self.placement_draft.is_none()
            && self.project_draft.is_none();
        ui.add_space(12.);
        ui.separator();
        ui.add_space(12.);
        let mut new = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("ANALYSIS").size(11.).color(MUTED))
                .on_hover_text(
                    "One section is active at a time: the selected visible section cuts the view.",
                );
            new = ui
                .add_enabled(idle, egui::Button::new("New section"))
                .on_hover_text("Save the current plane as a new section.")
                .clicked();
        });
        ui.add_space(10.);
        let mut show = None;
        let mut pick = None;
        for (id, section) in &self.data.sections {
            ui.horizontal(|ui| {
                let mut visible = section.visible;
                if ui
                    .add_enabled(idle, egui::Checkbox::new(&mut visible, "Show"))
                    .on_hover_text("Show or hide this section. Only the selected visible section cuts the view.")
                    .changed()
                {
                    show = Some((id.clone(), visible));
                }
                let selected = self.section_selected.as_deref() == Some(id.as_str());
                if ui
                    .add_enabled_ui(idle, |ui| ui.selectable_label(selected, id))
                    .inner
                    .clicked()
                {
                    pick = Some(id.clone());
                }
            });
        }
        if let Some(id) = pick {
            self.select_section(ui.ctx(), id);
        }
        let selected = self
            .section_selected
            .clone()
            .filter(|id| self.data.sections.contains_key(id));
        let mut flip = false;
        let mut delete = false;
        if selected.is_some() {
            ui.horizontal(|ui| {
                flip = ui
                    .add_enabled(idle, egui::Button::new("Flip"))
                    .on_hover_text("Reverse the kept side of the selected section.")
                    .clicked();
                delete = ui
                    .add_enabled(idle, egui::Button::new("Delete section"))
                    .clicked();
            });
        }
        if let Some((id, visible)) = show
            && let Some(section) = self.data.sections.get(&id)
        {
            let mut section = section.clone();
            section.visible = visible;
            self.save_sections(
                ui.ctx(),
                vec![faber_model::Edit::SetSection { id, section }],
            );
        }
        if flip
            && let Some(id) = &selected
            && let Some(section) = self.data.sections.get(id)
        {
            let mut section = section.clone();
            section.reverse = !section.reverse;
            self.save_sections(
                ui.ctx(),
                vec![faber_model::Edit::SetSection {
                    id: id.clone(),
                    section,
                }],
            );
        }
        if delete && let Some(id) = selected {
            self.save_sections(ui.ctx(), vec![faber_model::Edit::RemoveSection { id }]);
        }
        if new {
            let id = crate::analysis::new_id(&self.data);
            let section = crate::analysis::from_plane(self.plane);
            self.save_sections(
                ui.ctx(),
                vec![faber_model::Edit::SetSection { id, section }],
            );
        }
    }

    fn tree(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, create) in [("New project", true), ("Open project", false)] {
                if ui
                    .add_enabled(
                        self.pending.is_none()
                            && self.parameter_draft.is_none()
                            && self.feature_draft.is_none()
                            && self.sketch_draft.is_none()
                            && self.import_draft.is_none()
                            && self.placement_draft.is_none()
                            && self.project_draft.is_none(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.project_draft = Some(ProjectDraft {
                        create,
                        path: String::new(),
                        name: "Untitled".into(),
                    });
                    self.error = None;
                }
            }
        });
        ui.add_space(12.);
        ui.label(RichText::new("PROJECT").size(11.).color(MUTED));
        ui.add_space(12.);
        ui.label(RichText::new(&self.data.name).size(18.).strong());
        ui.add_space(4.);
        ui.label(
            RichText::new(format!(
                "{} solid{}",
                self.bodies.len(),
                if self.bodies.len() == 1 { "" } else { "s" }
            ))
            .color(MUTED),
        );
        ui.add_space(20.);
        ui.separator();
        ui.add_space(12.);
        ui.label(RichText::new("BODIES").size(11.).color(MUTED));
        ui.add_space(10.);
        for body in &self.bodies {
            ui.horizontal(|ui| {
                let mut visible = !self.hidden.contains(&body.id);
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Show body")
                    .changed()
                {
                    if visible {
                        self.hidden.remove(&body.id);
                    } else {
                        self.hidden.insert(body.id.clone());
                    }
                    self.mesh_dirty = true;
                }
                let selected = self.selected.as_deref() == Some(&body.id);
                if ui.selectable_label(selected, &body.name).clicked() {
                    self.selected = if selected {
                        None
                    } else {
                        Some(body.id.clone())
                    };
                    self.mesh_dirty = true;
                }
            });
        }
        ui.add_space(24.);
        if ui.button("Show all").clicked() {
            self.hidden.clear();
            self.mesh_dirty = true;
        }
        let part_id = self
            .selected
            .clone()
            .or_else(|| self.data.parts.keys().next().cloned());
        if ui
            .add_enabled(
                part_id.is_some()
                    && self.pending.is_none()
                    && self.parameter_draft.is_none()
                    && self.feature_draft.is_none()
                    && self.sketch_draft.is_none()
                    && self.import_draft.is_none()
                    && self.placement_draft.is_none()
                    && self.project_draft.is_none(),
                egui::Button::new("Move / rotate"),
            )
            .clicked()
            && let Some(id) = &part_id
            && let Some(part) = self.data.parts.get(id)
        {
            let text = |value: &faber_model::Scalar| match value {
                faber_model::Scalar::Number(number) => number.to_string(),
                faber_model::Scalar::Expression(expression) => expression.clone(),
            };
            self.placement_draft = Some(PlacementDraft {
                expected: self.data.clone(),
                part: id.clone(),
                translation: part.placement.translation.each_ref().map(text),
                rotation: part.placement.rotation.each_ref().map(text),
            });
            self.error = None;
        }
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("Linear pattern", ToolKind::Linear),
                ("Circular pattern", ToolKind::Circular),
                ("Mirror", ToolKind::Mirror),
                ("Shell", ToolKind::Shell),
                ("Fillet", ToolKind::Fillet),
                ("Chamfer", ToolKind::Chamfer),
                ("Offset solid", ToolKind::Offset),
                ("Draft", ToolKind::Draft),
                ("Thicken face", ToolKind::Thicken),
                ("Hole", ToolKind::Hole),
                ("Split body", ToolKind::Split),
            ] {
                if ui
                    .add_enabled(
                        part_id.is_some()
                            && self.pending.is_none()
                            && self.parameter_draft.is_none()
                            && self.feature_draft.is_none()
                            && self.sketch_draft.is_none()
                            && self.import_draft.is_none()
                            && self.placement_draft.is_none()
                            && self.project_draft.is_none(),
                        egui::Button::new(label),
                    )
                    .clicked()
                    && let Some(id) = &part_id
                    && let Err(error) = self.new_feature(id, kind)
                {
                    self.error = Some(error);
                }
            }
            ui.menu_button("References", |ui| {
                for (label, kind) in [
                    ("Reference plane", ToolKind::Plane),
                    ("Plane on face", ToolKind::FacePlane),
                    ("Reference axis", ToolKind::Axis),
                    ("Reference point", ToolKind::Point),
                ] {
                    if ui
                        .add_enabled(
                            part_id.is_some()
                                && self.pending.is_none()
                                && self.parameter_draft.is_none()
                                && self.feature_draft.is_none()
                                && self.sketch_draft.is_none()
                                && self.import_draft.is_none()
                                && self.placement_draft.is_none()
                                && self.project_draft.is_none(),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        if let Some(id) = &part_id
                            && let Err(error) = self.new_feature(id, kind)
                        {
                            self.error = Some(error);
                        }
                        ui.close();
                    }
                }
            });
        });
        let part = self
            .selected
            .as_ref()
            .and_then(|id| self.data.parts.get(id))
            .or_else(|| self.data.parts.values().next());
        if let Some(part) = part
            && let faber_model::Geometry::History { features, output } = &part.geometry
        {
            ui.add_space(20.);
            ui.separator();
            ui.add_space(12.);
            ui.label(RichText::new("FEATURES").size(11.).color(MUTED));
            egui::ScrollArea::vertical()
                .id_salt("feature_list")
                .max_height(220.)
                .show(ui, |ui| {
                    for (index, feature) in features.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:02}", index + 1))
                                    .small()
                                    .color(MUTED),
                            );
                            if ui
                                .add_enabled(
                                    self.pending.is_none()
                                        && self.parameter_draft.is_none()
                                        && self.sketch_draft.is_none()
                                        && self.import_draft.is_none()
                                        && self.placement_draft.is_none()
                                        && self.project_draft.is_none(),
                                    egui::Button::new(RichText::new(&feature.id).color(
                                        if &feature.id == output {
                                            ACCENT
                                        } else {
                                            Color32::LIGHT_GRAY
                                        },
                                    )),
                                )
                                .clicked()
                                && let Some(id) = &part_id
                            {
                                match crate::feature_form::FeatureForm::new(
                                    self.data.clone(),
                                    id.clone(),
                                    feature.id.clone(),
                                    &feature.geometry,
                                ) {
                                    Ok(form) => self.feature_draft = Some(form),
                                    Err(error) => self.error = Some(error),
                                }
                            }
                        });
                    }
                });
        }
        if !self.data.params.is_empty() {
            ui.add_space(20.);
            ui.separator();
            ui.add_space(12.);
            ui.label(RichText::new("PARAMETERS").size(11.).color(MUTED));
            if ui
                .add_enabled(
                    self.pending.is_none()
                        && self.feature_draft.is_none()
                        && self.sketch_draft.is_none()
                        && self.import_draft.is_none()
                        && self.placement_draft.is_none()
                        && self.project_draft.is_none(),
                    egui::Button::new("Edit parameters"),
                )
                .clicked()
            {
                self.parameter_draft = Some((self.data.clone(), self.data.params.clone()));
            }
            egui::ScrollArea::vertical()
                .id_salt("parameter_list")
                .max_height(180.)
                .show(ui, |ui| {
                    for (name, expression) in &self.data.params {
                        ui.label(format!("{name} = {expression}"));
                    }
                });
        }
        self.analysis_list(ui);
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.);
        ui.label(RichText::new("INSPECT").size(11.).color(MUTED));
        ui.add_space(16.);
        let body = self
            .selected
            .as_ref()
            .and_then(|id| self.bodies.iter().find(|b| &b.id == id))
            .or_else(|| self.bodies.first());
        if let Some(body) = body {
            ui.label(RichText::new(&body.name).size(18.).strong());
            ui.add_space(14.);
            let size: [f64; 3] = std::array::from_fn(|i| body.bounds[1][i] - body.bounds[0][i]);
            egui::Grid::new("dimensions")
                .spacing([22., 10.])
                .show(ui, |ui| {
                    for (name, value) in
                        [("Width", size[0]), ("Depth", size[1]), ("Height", size[2])]
                    {
                        ui.label(RichText::new(name).color(MUTED));
                        ui.label(format!("{value:.1} mm"));
                        ui.end_row();
                    }
                    ui.label(RichText::new("Solid volume").color(MUTED));
                    ui.label(format!("{:.1} cm³", body.volume / 1000.));
                    ui.end_row();
                });
            ui.add_space(16.);
            if let Some(part) = self.data.parts.get(&body.id)
                && !part.note.is_empty()
            {
                ui.label(RichText::new(&part.note).small().color(MUTED));
            }
        }
        ui.add_space(24.);
        ui.separator();
        ui.add_space(18.);
        ui.horizontal(|ui| {
            ui.label(RichText::new("SECTION ANALYSIS").size(11.).color(MUTED));
            if ui
                .checkbox(&mut self.section_on, "")
                .on_hover_text("Enable section")
                .changed()
            {
                self.mesh_dirty = true;
            }
        });
        ui.add_space(12.);
        let before = self.plane;
        ui.add_enabled_ui(self.section_on, |ui| {
            ui.horizontal(|ui| {
                for (label, n) in [
                    ("X", [1., 0., 0.]),
                    ("Y", [0., 1., 0.]),
                    ("Z", [0., 0., 1.]),
                ] {
                    if ui.selectable_label(self.plane.normal == n, label).clicked() {
                        self.plane.normal = n;
                        self.plane.offset = if n[2] == 1. {
                            self.camera.target.z as f64
                        } else {
                            0.
                        };
                        self.mesh_dirty = true;
                    }
                }
            });
            ui.add_space(10.);
            ui.label(RichText::new("Orientation").color(MUTED));
            let normal = glam::DVec3::from(self.plane.normal).normalize();
            let mut azimuth = normal.y.atan2(normal.x).to_degrees();
            let mut tilt = normal.z.clamp(-1., 1.).asin().to_degrees();
            let azimuth_changed = ui
                .add(
                    egui::Slider::new(&mut azimuth, -180. ..=180.)
                        .text("Azimuth")
                        .suffix("°"),
                )
                .changed();
            let tilt_changed = ui
                .add(
                    egui::Slider::new(&mut tilt, -90. ..=90.)
                        .text("Tilt")
                        .suffix("°"),
                )
                .changed();
            if azimuth_changed || tilt_changed {
                self.plane.normal = section_normal(azimuth, tilt);
                self.mesh_dirty = true;
            }
            let mut lo = 0f64;
            let mut hi = 1f64;
            for b in &self.bodies {
                for corner in 0..8 {
                    let d: f64 = (0..3)
                        .map(|i| {
                            b.bounds[(corner >> i) & 1][i]
                                * glam::DVec3::from(self.plane.normal).normalize()[i]
                        })
                        .sum();
                    lo = lo.min(d);
                    hi = hi.max(d);
                }
            }
            if ui
                .add(
                    egui::Slider::new(&mut self.plane.offset, lo - 1. ..=hi + 1.)
                        .suffix(" mm")
                        .text("Offset"),
                )
                .changed()
            {
                self.mesh_dirty = true;
            }
            if ui
                .checkbox(&mut self.plane.reverse, "Reverse direction")
                .changed()
            {
                self.mesh_dirty = true;
            }
        });
        if self.plane != before {
            self.section_selected = None;
        }
        ui.add_space(10.);
        ui.label(
            RichText::new("The cut changes the view only.")
                .small()
                .color(MUTED),
        );
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(12.);
            ui.label(
                RichText::new("F  Fit view     S  Section")
                    .small()
                    .color(MUTED),
            );
            ui.label(
                RichText::new("Drag to orbit · Scroll to zoom")
                    .small()
                    .color(MUTED),
            );
            ui.label(RichText::new("Right-drag to pan").small().color(MUTED));
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let delta = ui.input(|i| i.pointer.delta());
        if response.dragged_by(egui::PointerButton::Primary) {
            self.camera.yaw -= delta.x * 0.35;
            self.camera.pitch = (self.camera.pitch + delta.y * 0.35).clamp(-85., 89.);
            self.dirty = true;
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let forward = (self.camera.target - self.camera.eye()).normalize();
            let right = forward.cross(glam::Vec3::Z).normalize();
            let up = right.cross(forward);
            self.camera.target +=
                (-right * delta.x + up * delta.y) * self.camera.distance / rect.height();
            self.dirty = true;
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                self.camera.distance =
                    (self.camera.distance * (-scroll * 0.002).exp()).clamp(1., 1e7);
                self.dirty = true;
            }
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let point = [
                2. * (pos.x - rect.left()) / rect.width() - 1.,
                1. - 2. * (pos.y - rect.top()) / rect.height(),
            ];
            let (origin, direction) = self.camera.ray(rect.aspect_ratio(), point);
            let mut nearest = f64::INFINITY;
            let mut selected = None;
            for body in &self.bodies {
                if self.hidden.contains(&body.id) {
                    continue;
                }
                let distance = if self.section_on {
                    faber_scene::section(&body.mesh, self.plane)
                        .ok()
                        .and_then(|cut| {
                            [
                                faber_scene::pick(&cut.surface, origin, direction),
                                faber_scene::pick(&cut.cap, origin, direction),
                            ]
                            .into_iter()
                            .flatten()
                            .min_by(f64::total_cmp)
                        })
                } else {
                    faber_scene::pick(&body.mesh, origin, direction)
                };
                if let Some(distance) = distance
                    && distance < nearest
                {
                    nearest = distance;
                    selected = Some(body.id.clone());
                }
            }
            self.selected = selected;
            self.mesh_dirty = true;
        }
        let width = (rect.width() * ui.ctx().pixels_per_point())
            .round()
            .clamp(1., 4096.) as u32;
        let height = (rect.height() * ui.ctx().pixels_per_point())
            .round()
            .clamp(1., 4096.) as u32;
        if self
            .target
            .as_ref()
            .is_none_or(|t| t.width != width || t.height != height)
        {
            let target = self.gpu.target(width, height);
            let mut renderer = rs.renderer.write();
            match self.texture {
                None => {
                    self.texture = Some(renderer.register_native_texture(
                        &rs.device,
                        &target.display_view,
                        egui_wgpu::wgpu::FilterMode::Linear,
                    ))
                }
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    &target.display_view,
                    egui_wgpu::wgpu::FilterMode::Linear,
                    id,
                ),
            }
            self.target = Some(target);
            self.dirty = true;
        }
        if self.mesh_dirty {
            match self.gpu.upload(
                &self.bodies,
                &self.hidden,
                self.section_on.then_some(self.plane),
                self.selected.as_deref(),
            ) {
                Ok(list) => {
                    self.list = Some(list);
                }
                Err(e) => self.error = Some(e),
            }
            self.mesh_dirty = false;
            self.dirty = true;
        }
        if self.dirty
            && let (Some(target), Some(list)) = (&self.target, &self.list)
        {
            let start = Instant::now();
            self.gpu.draw(target, list, self.camera);
            self.frame_ms = start.elapsed().as_secs_f64() * 1000.;
            self.dirty = false;
        }
        if let Some(texture) = self.texture {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        let painter = ui.painter();
        painter.text(
            rect.left_top() + Vec2::new(24., 22.),
            egui::Align2::LEFT_TOP,
            if self.section_on {
                "SECTION VIEW"
            } else {
                "PERSPECTIVE"
            },
            egui::FontId::proportional(11.),
            MUTED,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(24., -20.),
            egui::Align2::LEFT_BOTTOM,
            "Z UP  /  mm",
            egui::FontId::monospace(11.),
            MUTED,
        );
        let centre = rect.right_bottom() + Vec2::new(-52., -52.);
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let target = matrix.project_point3(self.camera.target);
        for (label, axis, colour) in [
            ("X", glam::Vec3::X, Color32::from_rgb(225, 125, 120)),
            ("Y", glam::Vec3::Y, ACCENT),
            ("Z", glam::Vec3::Z, Color32::from_rgb(127, 173, 241)),
        ] {
            let p = matrix.project_point3(self.camera.target + axis * 20.) - target;
            let direction = Vec2::new(p.x, -p.y).normalized() * 25.;
            painter.line_segment([centre, centre + direction], egui::Stroke::new(2., colour));
            painter.text(
                centre + direction * 1.3,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(10.),
                colour,
            );
        }
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if !self.project_loaded {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.22).max(40.));
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(100., 80.), Sense::hover());
                    mark::paint(ui.painter(), rect.center(), 1.5, PANEL);
                    ui.add_space(8.);
                    ui.label(RichText::new("FABER").size(32.).strong());
                    ui.add_space(12.);
                    ui.label(
                        RichText::new("Build a design. Make it yours.")
                            .size(18.)
                            .color(MUTED),
                    );
                    ui.add_space(32.);
                    ui.add_enabled_ui(
                        self.pending.is_none() && self.project_draft.is_none(),
                        |ui| {
                            for (label, create) in [("New project", true), ("Open project", false)]
                            {
                                if ui
                                    .add_sized(
                                        [260., 48.],
                                        egui::Button::new(RichText::new(label).size(17.)),
                                    )
                                    .clicked()
                                {
                                    self.project_draft = Some(ProjectDraft {
                                        create,
                                        path: String::new(),
                                        name: "Untitled".into(),
                                    });
                                    self.error = None;
                                }
                                ui.add_space(12.);
                            }
                        },
                    );
                    ui.add_space(16.);
                    ui.label(
                        RichText::new("Sketch · Solid modelling · Section analysis").color(MUTED),
                    );
                });
            });
            self.project_panel(&ctx);
            return;
        }
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::F)) {
            self.fit();
        }
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::S)) {
            self.section_on = !self.section_on;
            self.mesh_dirty = true;
        }
        let panel = || {
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(egui::Margin::symmetric(18, 10))
        };
        let narrow = ui.available_width() < 1180.;
        egui::Panel::top("header")
            .exact_size(if narrow { 104. } else { 64. })
            .frame(panel())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(36.);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(24., 28.), Sense::hover());
                    mark::paint(ui.painter(), rect.center(), 0.54, PANEL);
                    ui.add_space(8.);
                    ui.label(RichText::new("FABER").size(18.).strong());
                    ui.add_space(28.);
                    ui.separator();
                    ui.add_space(16.);
                    ui.add_sized(
                        [150., 29.],
                        egui::Label::new(RichText::new(&self.data.name).color(MUTED)).truncate(),
                    )
                    .on_hover_text(&self.data.name);
                    if ui.button("Checks").clicked() {
                        self.checks_open = true;
                    }
                    if ui.button("Export").clicked() {
                        self.export_open = true;
                        if self.export_path.is_empty() {
                            self.export_path = self.root.join("out").to_string_lossy().into_owned();
                        }
                    }
                    if ui
                        .add_enabled(
                            self.pending.is_none()
                                && self.parameter_draft.is_none()
                                && self.feature_draft.is_none()
                                && self.sketch_draft.is_none()
                                && self.import_draft.is_none()
                                && self.placement_draft.is_none()
                                && self.project_draft.is_none(),
                            egui::Button::new("Import STEP"),
                        )
                        .clicked()
                    {
                        self.import_draft = Some(ImportDraft {
                            expected: self.data.clone(),
                            path: String::new(),
                            name: "Imported part".into(),
                        });
                        self.error = None;
                    }
                    if ui
                        .add_enabled(
                            self.pending.is_none()
                                && self.parameter_draft.is_none()
                                && self.feature_draft.is_none()
                                && self.sketch_draft.is_none()
                                && self.import_draft.is_none()
                                && self.placement_draft.is_none()
                                && self.project_draft.is_none(),
                            egui::Button::new("New sketch"),
                        )
                        .clicked()
                    {
                        self.sketch_draft =
                            Some(crate::sketch::SketchDraft::new(self.data.clone()));
                        self.error = None;
                    }
                    ui.add_enabled_ui(
                        self.pending.is_none()
                            && self.parameter_draft.is_none()
                            && self.feature_draft.is_none()
                            && self.sketch_draft.is_none()
                            && self.import_draft.is_none()
                            && self.placement_draft.is_none()
                            && self.project_draft.is_none(),
                        |ui| {
                            if ui.button("Undo").clicked() {
                                self.move_history(&ctx, false);
                            }
                            if ui.button("Redo").clicked() {
                                self.move_history(&ctx, true);
                            }
                        },
                    );
                    if !narrow {
                        self.view_controls(ui);
                    }
                });
                if narrow {
                    ui.horizontal(|ui| self.view_controls(ui));
                }
            });
        egui::Panel::bottom("status")
            .exact_size(32.)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(error) = &self.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    } else if self.saving {
                        ui.spinner();
                        ui.label("Building model…");
                    } else {
                        ui.label(RichText::new("Saved").color(ACCENT));
                    }
                    ui.label(
                        RichText::new(format!(
                            "  Revision {}   ·   {} triangles",
                            self.data.revision,
                            self.bodies
                                .iter()
                                .map(|b| b.mesh.triangles.len())
                                .sum::<usize>()
                        ))
                        .small()
                        .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.2} ms submit   ·   mm", self.frame_ms))
                                .small()
                                .color(MUTED),
                        );
                    });
                });
            });
        egui::Panel::left("tree")
            .exact_size(220.)
            .frame(panel())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("project_panel_scroll")
                    .show(ui, |ui| self.tree(ui));
            });
        egui::Panel::right("inspect")
            .exact_size(264.)
            .frame(panel())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inspect_panel_scroll")
                    .show(ui, |ui| self.inspector(ui));
            });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if let Some(rs) = frame.wgpu_render_state() {
                self.canvas(ui, rs);
            }
        });
        self.parameter_editor(&ctx);
        self.feature_editor(&ctx);
        self.sketch_editor(&ctx);
        self.checks(&ctx);
        self.export_panel(&ctx);
        self.import_panel(&ctx);
        self.placement_panel(&ctx);
        self.project_panel(&ctx);
    }
}

fn style(ctx: &egui::Context) {
    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.panel_fill = PANEL;
    style.visuals.override_text_color = Some(Color32::from_rgb(225, 232, 240));
    style.visuals.selection.bg_fill = Color32::from_rgb(39, 79, 80);
    style.visuals.selection.stroke = egui::Stroke::new(1., ACCENT);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 40, 51);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 40, 51);
    style.visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1., Color32::from_rgb(36, 46, 59));
    style.spacing.item_spacing = Vec2::new(8., 8.);
    style.spacing.button_padding = Vec2::new(12., 7.);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn periodic_spline_can_be_drawn_and_saved_from_the_interface() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let project =
            faber_wiring::open(directory.path(), true, Some("Closed spline GUI".into())).unwrap();
        let data = project.data.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, vec![], None));
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("New sketch").click();
        for _ in 0..8 {
            harness.step();
        }
        harness.get_by_label("Spline").click();
        for _ in 0..3 {
            harness.step();
        }
        let centre = harness.get_by_label("Sketch canvas").rect().center();
        for offset in [
            egui::vec2(20., 0.),
            egui::vec2(0., -20.),
            egui::vec2(-20., 0.),
            egui::vec2(0., 20.),
        ] {
            let pos = centre + offset;
            harness.event(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.step();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.query_by_label("Spline preview ready").is_none() && Instant::now() < deadline
        {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.get_by_label("Spline preview ready");
        harness.get_by_label("Closed spline").click();
        for _ in 0..8 {
            harness.step();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.query_by_label("Spline preview ready").is_none() && Instant::now() < deadline
        {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.get_by_label("Spline preview ready");
        harness.get_by_label("Create solid").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().sketch_draft.is_none());
        let (data, bodies) = faber_wiring::read(directory.path()).unwrap();
        let part = serde_json::to_value(data.parts.values().next().unwrap()).unwrap();
        let segments = &part["geometry"]["features"][0]["geometry"]["profile"]["outer"]["segments"];
        assert_eq!(segments.as_array().unwrap().len(), 1);
        assert_eq!(segments[0]["periodic"], serde_json::json!(true));
        assert!(bodies[0].volume > 8000. && bodies[0].volume < 16000.);
    }

    #[test]
    fn spline_can_be_drawn_trimmed_and_saved_from_the_interface() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let project =
            faber_wiring::open(directory.path(), true, Some("Spline GUI".into())).unwrap();
        let data = project.data.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, vec![], None));
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("New sketch").click();
        for _ in 0..8 {
            harness.step();
        }
        harness.get_by_label("Spline").click();
        harness.step();
        let centre = harness.get_by_label("Sketch canvas").rect().center();
        for offset in [
            egui::vec2(0., 0.),
            egui::vec2(10., -10.),
            egui::vec2(20., 0.),
            egui::vec2(30., 10.),
        ] {
            let pos = centre + offset;
            harness.event(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.step();
        }
        harness.get_by_label("Remove point").click();
        for _ in 0..8 {
            harness.step();
        }
        assert_eq!(
            harness.state().sketch_draft.as_ref().unwrap().points.len(),
            3
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.query_by_label("Spline preview ready").is_none() && Instant::now() < deadline
        {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.get_by_label("Spline preview ready");
        for _ in 0..8 {
            harness.step();
        }
        harness.get_by_label("Create solid").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().sketch_draft.is_none());
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        // The symmetric three-point parabola has area 100/3 square millimetres.
        assert!(
            (bodies[0].volume - 4000. / 3.).abs() < 1e-4,
            "Volume: {}",
            bodies[0].volume
        );
    }
    #[test]
    fn slot_can_be_drawn_and_saved_from_the_interface() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Sketch tools".into())).unwrap();
        project
            .set_params([("slot_length".into(), "30".into())].into())
            .unwrap();
        let data = project.data.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, vec![], None));
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("New sketch").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Slot").click();
        harness.step();
        let centre = harness.get_by_label("Sketch canvas").rect().center();
        for pos in [centre, centre + egui::vec2(80., -40.)] {
            harness.event(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.step();
        }
        harness.get_by_label("Length").focus();
        harness.step();
        harness.get_by_label("Length").type_text("slot_length");
        harness.step();
        harness.get_by_label("Width").focus();
        harness.step();
        harness.get_by_label("Width").type_text("10");
        harness.step();
        harness.get_by_label("Create solid").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().sketch_draft.is_none());
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        assert!(
            (bodies[0].volume - (200. + 25. * std::f64::consts::PI) * 40.).abs() < 1e-5,
            "Volume: {}",
            bodies[0].volume
        );
        let mut project = faber_wiring::open(directory.path(), false, None).unwrap();
        project
            .set_params([("slot_length".into(), "40".into())].into())
            .unwrap();
        assert!(
            (project.bodies[0].volume - (300. + 25. * std::f64::consts::PI) * 40.).abs() < 1e-5
        );
    }
    #[test]
    fn edge_tools_build_through_the_interface() {
        use egui_kittest::kittest::Queryable;
        for (tool, volume) in [
            ("Fillet", 6000. - (4. - std::f64::consts::PI) * 30.),
            ("Chamfer", 5940.),
            ("Thicken face", 200.),
            ("Reference plane", 6000.),
            ("Reference axis", 6000.),
            ("Reference point", 6000.),
            ("Plane on face", 6000.),
            ("Split body", 3000.),
            ("Hole", 6000. - 48. * std::f64::consts::PI),
            ("Counterbore", 6000. - 84. * std::f64::consts::PI),
            ("Countersink", 6000. - 176. / 3. * std::f64::consts::PI),
            ("Draft", 6000. - 9000. * 5_f64.to_radians().tan()),
            (
                "Offset solid",
                8200. + (60. + 4. / 3.) * std::f64::consts::PI,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut project =
                faber_wiring::open(directory.path(), true, Some("Edge tools".into())).unwrap();
            let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}})).unwrap();
            project.put_part("box".into(), part).unwrap();
            let data = project.data.clone();
            let bodies = project.bodies.clone();
            drop(project);
            let root = directory.path().to_owned();
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1440., 1100.))
                .wgpu_setup(crate::gpu_setup())
                .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
            for _ in 0..3 {
                harness.step();
            }
            let hole = matches!(tool, "Hole" | "Counterbore" | "Countersink");
            if tool.starts_with("Reference ") || tool == "Plane on face" {
                harness.get_by_label("References").click();
                for _ in 0..3 {
                    harness.step();
                }
            }
            harness
                .get_by_label(if hole { "Hole" } else { tool })
                .click();
            for _ in 0..8 {
                harness.step();
            }
            if hole {
                let form = harness.state_mut().feature_draft.as_mut().unwrap();
                for (path, value) in [
                    ("/diameter", "2 * 2"),
                    ("/depth", "6 * 2"),
                    ("/placement/translation/0", "5"),
                    ("/placement/translation/1", "10"),
                    ("/placement/translation/2", "30"),
                    ("/placement/rotation/0", "180"),
                ] {
                    form.fields
                        .iter_mut()
                        .find(|field| field.0 == path)
                        .unwrap()
                        .1 = value.into();
                }
                harness
                    .get_by_label(if tool == "Hole" { "Counterbore" } else { tool })
                    .click();
                for _ in 0..8 {
                    harness.step();
                }
                if tool == "Hole" {
                    harness.get_by_label("Plain").click();
                    for _ in 0..8 {
                        harness.step();
                    }
                }
                let geometry = harness
                    .state()
                    .feature_draft
                    .as_ref()
                    .unwrap()
                    .geometry()
                    .unwrap();
                let value = serde_json::to_value(geometry).unwrap();
                assert_eq!(value["depth"], "6 * 2");
                assert_eq!(value["diameter"], "2 * 2");
            }
            if tool == "Split body" {
                let form = harness.state_mut().feature_draft.as_mut().unwrap();
                form.fields
                    .iter_mut()
                    .find(|field| field.0 == "/origin/2")
                    .unwrap()
                    .1 = "30 / 2".into();
                harness.get_by_label("Negative side").click();
                for _ in 0..8 {
                    harness.step();
                }
            }
            harness.get_by_label("Apply feature").click();
            harness.step();
            let deadline = Instant::now() + Duration::from_secs(10);
            while harness.state().saving && Instant::now() < deadline {
                harness.step();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                harness.state().error.is_none(),
                "{:?}",
                harness.state().error
            );
            assert!(harness.state().feature_draft.is_none());
            let (data, bodies) = faber_wiring::read(directory.path()).unwrap();
            if let Some(kind) = tool
                .strip_prefix("Reference ")
                .or_else(|| (tool == "Plane on face").then_some("plane_on_face"))
            {
                let saved = serde_json::to_value(&data.parts["box"].geometry).unwrap();
                assert_eq!(saved["output"], "source");
                assert_eq!(saved["features"][1]["geometry"]["op"], kind);
            }
            assert!(
                (bodies[0].volume - volume).abs() < 1e-5,
                "{tool}: {}",
                bodies[0].volume
            );
        }
    }
    #[test]
    fn shell_tool_builds_an_open_box_through_the_interface() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Shell GUI".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let data = project.data.clone();
        let bodies = project.bodies.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Shell").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().feature_draft.is_none());
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        assert!((bodies[0].volume - 1824.).abs() < 1e-5);
    }
    #[test]
    fn short_window_can_scroll_to_project_parameters() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Panel test".into())).unwrap();
        project
            .set_params([("width".into(), "10".into())].into())
            .unwrap();
        let features: Vec<_> = (0..8).map(|index| serde_json::json!({"id":format!("feature_{index}"),"geometry":{"op":"box","size":["width",10,10]}})).collect();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"history","features":features,"output":"feature_7"}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let data = project.data.clone();
        let bodies = project.bodies.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000., 650.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..3 {
            harness.step();
        }
        assert!(
            !harness
                .get_by_label("Redo")
                .rect()
                .intersects(harness.get_by_label("Iso").rect()),
            "Header controls overlap"
        );
        harness.event(egui::Event::PointerMoved(egui::pos2(100., 180.)));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., -1200.),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::NONE,
        });
        for _ in 0..10 {
            harness.step();
        }
        let button = harness.get_by_label("Edit parameters");
        assert!(
            button.rect().bottom() < 630.,
            "Parameter button is below the panel: {:?}",
            button.rect()
        );
        button.click();
        harness.step();
        assert!(harness.state().parameter_draft.is_some());
    }
    #[test]
    fn welcome_screen_creates_a_project_without_a_window() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("design");
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1200., 800.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(|cc| Shell::welcome(cc));
        for _ in 0..3 {
            harness.step();
        }
        harness.render().unwrap();
        assert!(harness.query_by_label("New sketch").is_none());
        harness.get_by_label("New project").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Project folder").focus();
        harness.step();
        harness
            .get_by_label("Project folder")
            .type_text(&path.to_string_lossy());
        harness.step();
        harness.get_by_label("Load project").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert_eq!(harness.state().root, path);
        assert!(path.join("faber.json").is_file());
        harness.get_by_label("New sketch");
        harness.render().unwrap();
    }
    use egui_kittest::kittest::Queryable;
    use faber_model::{Geometry, Part, Placement, Profile};

    #[test]
    fn section_angles_define_a_unit_normal_on_base_and_oblique_planes() {
        for (azimuth, tilt, expected) in [
            (0., 0., [1., 0., 0.]),
            (90., 0., [0., 1., 0.]),
            (0., 90., [0., 0., 1.]),
            (45., 0., [0.5_f64.sqrt(), 0.5_f64.sqrt(), 0.]),
        ] {
            let normal = section_normal(azimuth, tilt);
            for i in 0..3 {
                assert!((normal[i] - expected[i]).abs() < 1e-12);
            }
            assert!((normal.iter().map(|x| x * x).sum::<f64>() - 1.).abs() < 1e-12);
        }
    }

    #[test]
    fn actual_viewer_renders_sections_and_loads_saved_changes_without_a_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Viewer test".into())).unwrap();
        let mut part = Part {
            name: "Bin".into(),
            geometry: Geometry::Pocket {
                profile: Profile {
                    outer: vec![[-50., -50.], [50., -50.], [50., 50.], [-50., 50.]],
                    holes: vec![vec![[-46., -46.], [46., -46.], [46., 46.], [-46., 46.]]],
                },
                height: 120.,
                floor: 4.,
            },
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        part.geometry = Geometry::History {
            features: vec![faber_model::Feature {
                id: "walls".into(),
                geometry: part.geometry,
            }],
            output: "walls".into(),
        };
        project
            .set_params([("wall".into(), "4".into())].into())
            .unwrap();
        project.put_part("bin".into(), part.clone()).unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        let exterior = harness.render().unwrap();
        harness.get_by_label("Export").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Write files").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().export_pending.is_some() && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(harness.state().export_pending.is_none());
        assert!(
            harness
                .state()
                .export_message
                .as_ref()
                .unwrap()
                .starts_with("Exported 1 files")
        );
        assert!(dir.path().join("out/bin.step").exists());
        harness.state_mut().export_open = false;
        harness.step();
        harness.get_by_label("FEATURES");
        harness.get_by_label("walls");
        harness.get_by_label("PARAMETERS");
        harness.get_by_label("wall = 4");
        assert!(harness.state().error.is_none());
        let target = harness.state().target.as_ref().unwrap();
        let native = harness.state().gpu.read(target).unwrap();
        let at = ((500 - 64) * target.width + (700 - 220)) as usize * 4;
        let shown = exterior.get_pixel(700, 500);
        for channel in 0..3 {
            assert!(
                shown[channel].abs_diff(native[at + channel]) <= 3,
                "Viewer colour differs from the renderer: {shown:?} vs {:?}",
                &native[at..at + 4]
            );
        }
        harness.key_press(egui::Key::S);
        for _ in 0..3 {
            harness.step();
        }
        assert!(harness.state().section_on);
        let saved_normal = harness.state().plane.normal;
        harness
            .get_by_role_and_label(egui::Role::Slider, "Azimuth")
            .set_value_accesskit(45.);
        harness.step();
        harness
            .get_by_role_and_label(egui::Role::Slider, "Tilt")
            .set_value_accesskit(30.);
        harness.step();
        let expected = section_normal(45., 30.);
        for (actual, expected) in harness.state().plane.normal.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        harness.render().unwrap();
        assert!(harness.state().error.is_none());
        harness.state_mut().plane.normal = saved_normal;
        harness.state_mut().mesh_dirty = true;
        harness.step();
        let cut = harness.render().unwrap();
        let mut changed = 0;
        for y in 100..800 {
            for x in 300..1100 {
                if exterior.get_pixel(x, y) != cut.get_pixel(x, y) {
                    changed += 1;
                }
            }
        }
        assert!(changed > 1000, "Section did not change the viewport");
        part.colour = [210, 130, 80];
        project.put_part("bin".into(), part).unwrap();
        harness.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().data.revision != 3 && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(harness.state().data.revision, 3);
        assert_eq!(harness.state().bodies[0].colour, [210, 130, 80]);
        harness.key_press(egui::Key::S);
        harness.step();
        assert!(!harness.state().section_on);
        harness.get_by_label("Checks").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Run interference check").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().check_pending.is_some() && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(harness.state().check_pending.is_none());
        assert!(
            harness
                .state()
                .check_result
                .as_ref()
                .unwrap()
                .1
                .violations
                .is_empty()
        );
        harness.step();
        harness.get_by_label("No mesh interference or clearance findings.");
        let name = harness.state().data.name.clone();
        harness.state_mut().data.name = "Changed".into();
        harness.step();
        harness.get_by_label("Design changed. Run the check again.");
        harness.state_mut().data.name = name;
        harness.state_mut().checks_open = false;
        harness.step();
        drop(project);
        harness.get_by_label("Edit parameters").click();
        for _ in 0..3 {
            harness.step();
        }
        harness
            .state_mut()
            .parameter_draft
            .as_mut()
            .unwrap()
            .1
            .insert("wall".into(), "6".into());
        harness.get_by_label("Apply parameters").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!harness.state().saving);
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().parameter_draft.is_none());
        assert_eq!(harness.state().data.params["wall"], "6");
        assert_eq!(
            faber_wiring::read(dir.path()).unwrap().0.params["wall"],
            "6"
        );
        for (button, value) in [("Undo", "4"), ("Redo", "6")] {
            harness.get_by_label(button).click();
            harness.step();
            let deadline = Instant::now() + Duration::from_secs(10);
            while harness.state().saving && Instant::now() < deadline {
                harness.step();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(!harness.state().saving);
            assert!(
                harness.state().error.is_none(),
                "{:?}",
                harness.state().error
            );
            assert_eq!(harness.state().data.params["wall"], value);
            assert_eq!(
                faber_wiring::read(dir.path()).unwrap().0.params["wall"],
                value
            );
        }
        let previous_volume = harness.state().bodies[0].volume;
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().pending.is_some() && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("walls").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("height").focus();
        harness.step();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.get_by_label("height").type_text("160");
        harness.step();
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().feature_draft.is_none());
        assert!(harness.state().bodies[0].volume > previous_volume);
        assert!((harness.state().bodies[0].bounds[1][2] - 160.).abs() < 1e-5);
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert!((reopened[0].volume - harness.state().bodies[0].volume).abs() < 1e-6);
        let saved = std::fs::read(dir.path().join("faber.json")).unwrap();
        harness.get_by_label("walls").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("height").focus();
        harness.step();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.get_by_label("height").type_text("-1");
        harness.step();
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(harness.state().error.is_some());
        assert!(harness.state().feature_draft.is_some());
        assert_eq!(std::fs::read(dir.path().join("faber.json")).unwrap(), saved);
        assert!((harness.state().bodies[0].bounds[1][2] - 160.).abs() < 1e-5);
        harness.get_by_label("Cancel feature").click();
        harness.step();
        harness.get_by_label("New sketch").click();
        for _ in 0..3 {
            harness.step();
        }
        let centre = harness.get_by_label("Sketch canvas").rect().center();
        for pos in [centre, centre + egui::vec2(40., -60.)] {
            harness.event(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.step();
        }
        harness.get_by_label("Create solid").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().sketch_draft.is_none());
        let body = harness
            .state()
            .bodies
            .iter()
            .find(|b| b.id == "body_1")
            .unwrap();
        assert!((body.volume - 24000.).abs() < 1e-6);
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert!((reopened.iter().find(|b| b.id == "body_1").unwrap().volume - 24000.).abs() < 1e-6);
        harness.state_mut().selected = Some("body_1".into());
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("Linear pattern").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().feature_draft.is_none());
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert!((reopened.iter().find(|b| b.id == "body_1").unwrap().volume - 48000.).abs() < 1e-6);
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("Circular pattern").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().feature_draft.is_none());
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert!((reopened.iter().find(|b| b.id == "body_1").unwrap().volume - 96000.).abs() < 1e-6);
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("Mirror").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("keep source").click();
        harness.step();
        harness.get_by_label("keep source").click();
        harness.step();
        harness.get_by_label("Apply feature").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert!(harness.state().feature_draft.is_none());
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert!(
            (reopened.iter().find(|b| b.id == "body_1").unwrap().volume - 192000.).abs() < 1e-6
        );
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("Import STEP").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Source file").focus();
        harness.step();
        harness
            .get_by_label("Source file")
            .type_text(&dir.path().join("out/bin.step").to_string_lossy());
        harness.step();
        harness.get_by_label("Import solid").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        let (_, reopened) = faber_wiring::read(dir.path()).unwrap();
        assert_eq!(reopened.len(), 3);
        let original = reopened.iter().find(|body| body.id == "body_2").unwrap();
        let original_bounds = original.bounds;
        let original_volume = original.volume;
        harness.state_mut().selected = Some("body_2".into());
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("Move / rotate").click();
        for _ in 0..3 {
            harness.step();
        }
        for (label, value) in [("Position X", "500"), ("Rotation Z", "90")] {
            harness.get_by_label(label).focus();
            harness.step();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.get_by_label(label).type_text(value);
            harness.step();
        }
        harness.get_by_label("Apply placement").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.state().saving && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        let (_, placed) = faber_wiring::read(dir.path()).unwrap();
        let moved = placed.iter().find(|body| body.id == "body_2").unwrap();
        assert!((moved.volume - original_volume).abs() < 1e-5);
        assert!((moved.bounds[0][0] - (500. - original_bounds[1][1])).abs() < 1e-5);
        let original_file = std::fs::read(dir.path().join("faber.json")).unwrap();
        let next_directory = tempfile::tempdir().unwrap();
        let new_path = next_directory.path().join("new_project");
        for (button, path, count) in [
            ("New project", new_path.as_path(), 0),
            ("Open project", dir.path(), 3),
        ] {
            harness.state_mut().last_poll = Instant::now();
            harness.step();
            harness.get_by_label(button).click();
            for _ in 0..3 {
                harness.step();
            }
            harness.get_by_label("Project folder").focus();
            harness.step();
            harness
                .get_by_label("Project folder")
                .type_text(&path.to_string_lossy());
            harness.step();
            harness.get_by_label("Load project").click();
            harness.step();
            let deadline = Instant::now() + Duration::from_secs(10);
            while harness.state().saving && Instant::now() < deadline {
                harness.step();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                harness.state().error.is_none(),
                "{:?}",
                harness.state().error
            );
            assert_eq!(harness.state().bodies.len(), count);
            assert_eq!(harness.state().root, path);
            assert!(harness.state().selected.is_none());
        }
        assert_eq!(
            std::fs::read(dir.path().join("faber.json")).unwrap(),
            original_file
        );
        assert!(
            reopened
                .iter()
                .any(|body| body.id == "body_2" && body.volume > 0.)
        );
    }

    #[test]
    fn header_and_start_screen_show_the_mark() {
        use egui_kittest::kittest::Queryable;
        let directory = tempfile::tempdir().unwrap();
        let project = faber_wiring::open(directory.path(), true, Some("Mark test".into())).unwrap();
        let data = project.data.clone();
        let bodies = project.bodies.clone();
        drop(project);
        let root = directory.path().to_owned();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 300.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("FABER");
        harness
            .render()
            .unwrap()
            .save(shot_dir().join("header-mark.png"))
            .unwrap();

        let mut start = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 700.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(|cc| Shell::welcome(cc));
        for _ in 0..3 {
            start.step();
        }
        start.get_by_label("FABER");
        start
            .render()
            .unwrap()
            .save(shot_dir().join("start-mark.png"))
            .unwrap();
    }

    /// Waits until the save and the reload that follows it are done.
    fn wait_for_save(harness: &mut egui_kittest::Harness<'_, Shell>) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            harness.step();
            let state = harness.state();
            let file = std::fs::metadata(state.root.join("faber.json"))
                .and_then(|m| m.modified())
                .ok();
            if state.pending.is_none() && state.modified == file {
                break;
            }
            assert!(Instant::now() < deadline, "Save did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
    }

    fn shot_dir() -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn saved_json(root: &std::path::Path) -> serde_json::Value {
        let text = std::fs::read_to_string(root.join("faber.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn analysis_folder_lists_hides_selects_flips_and_saves_sections() {
        use egui_kittest::kittest::Queryable;
        let dir = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(dir.path(), true, Some("Analysis test".into())).unwrap();
        let geometry = Geometry::Pocket {
            profile: Profile {
                outer: vec![[-50., -50.], [50., -50.], [50., 50.], [-50., 50.]],
                holes: vec![vec![[-46., -46.], [46., -46.], [46., 46.], [-46., 46.]]],
            },
            height: 40.,
            floor: 4.,
        };
        let part = Part {
            name: "Bin".into(),
            geometry: Geometry::History {
                features: vec![faber_model::Feature {
                    id: "walls".into(),
                    geometry,
                }],
                output: "walls".into(),
            },
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("bin".into(), part).unwrap();
        drop(project);
        let path = dir.path().join("faber.json");
        let mut json = saved_json(dir.path());
        json["sections"] = serde_json::json!({
            "side": {"plane": {"kind": "plane", "origin": [0, 0, 0], "normal": [0, 1, 0]}}
        });
        std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        assert!(data.sections.contains_key("side"));
        let root = dir.path().to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1500.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.get_by_label("ANALYSIS");
        harness.get_by_label("side");
        harness
            .render()
            .unwrap()
            .save(shot_dir().join("analysis-panel-initial.png"))
            .unwrap();

        // The eye hides the section and saves the change.
        harness
            .get_by_role_and_label(egui::Role::CheckBox, "Show")
            .click();
        wait_for_save(&mut harness);
        assert!(!harness.state().data.sections["side"].visible);
        assert_eq!(saved_json(dir.path())["sections"]["side"]["visible"], false);
        harness
            .get_by_role_and_label(egui::Role::CheckBox, "Show")
            .click();
        wait_for_save(&mut harness);
        assert!(harness.state().data.sections["side"].visible);

        // Selecting the section sets the view plane from it.
        harness.get_by_label("side").click();
        for _ in 0..3 {
            harness.step();
        }
        assert_eq!(harness.state().section_selected.as_deref(), Some("side"));
        assert!(harness.state().section_on);
        assert_eq!(harness.state().plane.normal, [0., 1., 0.]);
        assert_eq!(harness.state().plane.offset, 0.);
        assert!(!harness.state().plane.reverse);

        // Flip saves reverse as true.
        harness.get_by_label("Flip").click();
        wait_for_save(&mut harness);
        assert_eq!(saved_json(dir.path())["sections"]["side"]["reverse"], true);
        assert!(harness.state().plane.reverse);
        harness
            .render()
            .unwrap()
            .save(shot_dir().join("analysis-panel-selected.png"))
            .unwrap();

        // A plane change clears the analysis selection. The offset moves the plane.
        harness.get_by_label("X").click();
        harness.step();
        assert_eq!(harness.state().section_selected, None);
        harness
            .get_by_role_and_label(egui::Role::Slider, "Offset")
            .set_value_accesskit(12.);
        harness.step();

        // The new section button saves the current plane.
        harness.get_by_label("New section").click();
        wait_for_save(&mut harness);
        let saved = saved_json(dir.path());
        let sections = saved["sections"].as_object().unwrap();
        assert_eq!(sections.len(), 2, "{sections:?}");
        let new = sections
            .iter()
            .find(|(id, _)| id.as_str() != "side")
            .unwrap()
            .1;
        assert_eq!(new["plane"]["kind"], "plane");
        assert_eq!(new["plane"]["normal"], serde_json::json!([1.0, 0.0, 0.0]));
        assert_eq!(new["plane"]["origin"], serde_json::json!([12.0, 0.0, 0.0]));
        assert_eq!(new["visible"], true);

        // Delete removes the selected section from the file.
        harness.get_by_label("side").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Delete section").click();
        wait_for_save(&mut harness);
        assert!(saved_json(dir.path())["sections"].get("side").is_none());
        assert!(!harness.state().data.sections.contains_key("side"));
    }

    #[test]
    fn analysis_reference_section_resolves_through_the_model() {
        use egui_kittest::kittest::Queryable;
        let dir = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(dir.path(), true, Some("Reference test".into())).unwrap();
        let pocket = Geometry::Pocket {
            profile: Profile {
                outer: vec![[-50., -50.], [50., -50.], [50., 50.], [-50., 50.]],
                holes: vec![vec![[-46., -46.], [46., -46.], [46., 46.], [-46., 46.]]],
            },
            height: 40.,
            floor: 4.,
        };
        let part = Part {
            name: "Bin".into(),
            geometry: Geometry::History {
                features: vec![
                    faber_model::Feature {
                        id: "walls".into(),
                        geometry: pocket,
                    },
                    faber_model::Feature {
                        id: "ref".into(),
                        geometry: Geometry::Plane {
                            placement: Placement {
                                translation: [0., 0., 10.],
                                rotation: [0., 0., 0.],
                            },
                        },
                    },
                ],
                output: "walls".into(),
            },
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("bin".into(), part).unwrap();
        drop(project);
        let path = dir.path().join("faber.json");
        let mut json = saved_json(dir.path());
        json["sections"] = serde_json::json!({
            "lid_plane": {"plane": {"kind": "reference", "part": "bin", "feature": "ref", "offset": 5}}
        });
        std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1500.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.get_by_label("lid_plane").click();
        let deadline = Instant::now() + Duration::from_secs(20);
        harness.step();
        while harness.state().section_pending.is_some() && Instant::now() < deadline {
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.step();
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
        assert_eq!(
            harness.state().section_selected.as_deref(),
            Some("lid_plane")
        );
        assert!(harness.state().section_on);
        let plane = harness.state().plane;
        for (actual, expected) in plane.normal.iter().zip([0., 0., 1.]) {
            assert!((actual - expected).abs() < 1e-9, "{:?}", plane.normal);
        }
        assert!((plane.offset - 15.).abs() < 1e-6, "offset {}", plane.offset);
    }
}
