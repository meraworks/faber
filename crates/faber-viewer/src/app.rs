mod windows;
use windows::{Draft, ToolWindow, WindowEvent, WindowState, tool_window};
mod assembly;
#[cfg(test)]
mod camera_tests;
mod command_search;
#[cfg(test)]
mod controls_fixture;
mod dialogs;
#[cfg(test)]
mod edge_pick_tests;
mod face_pick;
use face_pick::{Pick, SelectFilter};
#[cfg(test)]
mod face_pick_tests;
mod feature_tools;
#[cfg(test)]
mod forms_tests;
mod keys;
#[cfg(test)]
mod keys_tests;
mod marking_menu;
mod material;
#[cfg(test)]
mod material_tests;
mod motion_relations;
mod move_tool;
mod pick_tools;
#[cfg(test)]
mod pick_tools_tests;
mod press_pull;
#[cfg(test)]
mod press_pull_tests;
mod selection;
#[cfg(test)]
mod selection_tests;
mod study;
#[cfg(test)]
mod suppress_tests;
mod view_cube;
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

// The candidate nearest the view centre that is inside the inner rect and the depth range.
fn visible_anchor(
    candidates: impl Iterator<Item = glam::Vec3>,
    matrix: glam::Mat4,
    rect: egui::Rect,
    inner: egui::Rect,
) -> Option<glam::Vec3> {
    candidates
        .filter_map(|point| {
            let seen = matrix.project_point3(point);
            let at =
                rect.center() + Vec2::new(seen.x * rect.width(), -seen.y * rect.height()) * 0.5;
            let visible = (0.0..=1.0).contains(&seen.z) && inner.contains(at);
            visible.then(|| (at.distance_sq(rect.center()), point))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

// Centres of the cut-face triangles of the visible bodies. The caps are in world space.
// At most about 4000 triangles are visited.
fn cut_centroids(
    caps: Option<&DrawList>,
    bodies: &[Body],
    hidden: &BTreeSet<String>,
) -> Vec<glam::Vec3> {
    let Some(caps) = caps else {
        return Vec::new();
    };
    let meshes: Vec<&faber_ports::Mesh> = bodies
        .iter()
        .filter(|body| !hidden.contains(&body.id))
        .filter_map(|body| caps.cap_mesh(&body.id))
        .collect();
    let total: usize = meshes.iter().map(|mesh| mesh.triangles.len()).sum();
    let stride = total.div_ceil(4000).max(1);
    meshes
        .iter()
        .flat_map(|mesh| mesh.triangles.iter().map(move |tri| (*mesh, tri)))
        .step_by(stride)
        .map(|(mesh, tri)| {
            let point = |i: u32| glam::Vec3::from(mesh.vertices[i as usize].map(|v| v as f32));
            (point(tri[0]) + point(tri[1]) + point(tri[2])) / 3.
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum SectionDrag {
    Move {
        axis: Vec2,
        scale: f32,
        offset: f64,
    },
    Rotate {
        index: usize,
        axis: glam::Vec3,
        normal: glam::Vec3,
        pivot: glam::Vec3,
        rate: Vec2,
        basis: glam::Vec3,
    },
}

type Loaded = Result<(ProjectData, Vec<Body>), String>;
type Exported = Result<(u64, Vec<String>), String>;

struct ImportDraft {
    bought: bool,
    source: String,
    licence: String,
    designation: String,
    expected: ProjectData,
    path: String,
    name: String,
}

struct PlacementDraft {
    expected: ProjectData,
    part: String,
    translation: [String; 3],
    rotation: [String; 3],
    // Other selected parts. When not empty, the values are relative moves of the group.
    others: Vec<String>,
}

/// Three fields with zero.
fn zero_fields() -> [String; 3] {
    std::array::from_fn(|_| "0".to_owned())
}

#[cfg(not(test))]
fn is_archive_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("f3d"))
}

struct ProjectDraft {
    create: bool,
    path: String,
    name: String,
}

#[derive(Clone, Copy)]
pub(crate) enum ToolKind {
    Plane,
    FacePlane,
    Axis,
    Point,
    AnglePlane,
    Midplane,
    PointsPlane,
    CylinderAxis,
    PlanesAxis,
    CentrePoint,
    Linear,
    Circular,
    Mirror,
    Shell,
    Scale,
    Pipe,
    PathPattern,
    Fillet,
    Chamfer,
    Offset,
    Draft,
    Thicken,
    Hole,
    Thread,
    Split,
}

/// Ids of the part features that pass `keep`. The newest feature comes first.
fn reference_ids(
    part: &faber_model::Part<faber_model::Scalar>,
    keep: fn(&faber_model::Geometry<faber_model::Scalar>) -> bool,
) -> Vec<String> {
    match &part.geometry {
        faber_model::Geometry::History { features, .. } => features
            .iter()
            .rev()
            .filter(|feature| keep(&feature.geometry))
            .map(|feature| feature.id.clone())
            .collect(),
        _ => vec![],
    }
}

fn is_plane(geometry: &faber_model::Geometry<faber_model::Scalar>) -> bool {
    use faber_model::Geometry;
    matches!(
        geometry,
        Geometry::Plane { .. }
            | Geometry::PlaneOnFace { .. }
            | Geometry::PlaneAtAngle { .. }
            | Geometry::Midplane { .. }
            | Geometry::PlaneThroughPoints { .. }
    )
}

fn is_axis(geometry: &faber_model::Geometry<faber_model::Scalar>) -> bool {
    use faber_model::Geometry;
    matches!(
        geometry,
        Geometry::Axis { .. } | Geometry::AxisFromCylinder { .. } | Geometry::AxisFromPlanes { .. }
    )
}

pub struct Shell {
    study: study::StudyPanel,
    window: WindowState,
    search: command_search::CommandSearch,
    marking: marking_menu::MarkingMenu,
    deleting: Option<String>,
    delete_notice: Option<(String, Instant)>,
    project_loaded: bool,
    recent_file: Option<PathBuf>,
    dialog: Option<dialogs::PendingDialog>,
    new_parameter: [String; 2],
    recent: Vec<(String, PathBuf)>,
    motion: assembly::Controls,
    root: PathBuf,
    press: Option<press_pull::Tool>,
    pub data: ProjectData,
    pub bodies: std::sync::Arc<Vec<Body>>,
    pub camera: Camera,
    pub section_on: bool,
    overhang: crate::overhang::State,
    pub plane: Section,
    pub selected: Option<String>,
    // Other selected parts. The primary part is `selected`.
    extra_selected: BTreeSet<String>,
    // Press point of a rectangle selection on empty space.
    box_drag: Option<egui::Pos2>,
    hover_pos: Option<egui::Pos2>,
    select_filter: SelectFilter,
    picks: Vec<Pick>,
    pick_point: Option<[f64; 3]>,
    hover_pick: Option<Pick>,
    hover_body: Option<String>,
    pick_info: face_pick::InfoCache,
    reveal_features: bool,
    copied_placement: Option<(String, faber_model::Placement<faber_model::Scalar>)>,
    pub section_selected: Option<String>,
    section_pending: Option<mpsc::Receiver<(String, Result<Section, String>)>>,
    pub hidden: BTreeSet<String>,
    gpu: Gpu,
    target: Option<Target>,
    texture: Option<egui::TextureId>,
    list: Option<DrawList>,
    caps: Option<DrawList>,
    cut_indices: std::sync::Arc<Vec<faber_scene::SectionIndex>>,
    cap_plane: Option<Section>,
    section_drag: Option<SectionDrag>,
    section_basis: Option<glam::Vec3>,
    section_pivot: Option<glam::Vec3>,
    // World point where the section handle sits.
    section_anchor: Option<glam::Vec3>,
    dirty: bool,
    mesh_dirty: bool,
    pub error: Option<String>,
    last_poll: Instant,
    modified: Option<(SystemTime, u64)>,
    pending: Option<mpsc::Receiver<Loaded>>,
    frame_ms: f64,
    parameter_draft: Option<(ProjectData, BTreeMap<String, String>)>,
    saving: bool,
    pending_pick_action: Option<pick_tools::PendingPick>,
    pick_tool_filter: Option<SelectFilter>,
    feature_draft: Option<crate::feature_form::FeatureForm>,
    sketch_draft: Option<crate::sketch::SketchDraft>,
    import_draft: Option<ImportDraft>,
    placement_draft: Option<PlacementDraft>,
    move_draft: Option<move_tool::MoveDraft>,
    project_draft: Option<ProjectDraft>,
    #[cfg(test)]
    picked_file: Option<PathBuf>,
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
    pub fn load_recent(&mut self, file: PathBuf) {
        self.recent = crate::recent::read(&file).unwrap_or_default();
        self.recent_file = Some(file);
        if self.project_loaded {
            self.remember_project();
        }
    }

    fn remember_project(&mut self) {
        if let Some(file) = &self.recent_file {
            match crate::recent::record(file, &self.data.name, &self.root) {
                Ok(entries) => self.recent = entries,
                Err(error) => self.error = Some(format!("Cannot save recent projects: {error}")),
            }
        }
    }

    pub fn welcome(cc: &eframe::CreationContext<'_>) -> Self {
        let data = ProjectData {
            studies: Default::default(),
            materials: Default::default(),
            schema: 1,
            revision: 0,
            name: String::new(),
            parts: Default::default(),
            params: Default::default(),
            contacts: Default::default(),
            motion: Default::default(),
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
        let modified = faber_wiring::project_fingerprint(&root).ok();
        Self {
            project_loaded: true,
            study: Default::default(),
            recent_file: None,
            dialog: None,
            new_parameter: Default::default(),
            recent: Vec::new(),
            motion: Default::default(),
            root,
            data,
            press: None,
            bodies: bodies.into(),
            camera,
            section_on: plane.is_some(),
            overhang: Default::default(),
            plane: plane.unwrap_or(Section {
                normal: [1., 0., 0.],
                offset: 0.,
                reverse: false,
            }),
            selected: None,
            extra_selected: BTreeSet::new(),
            box_drag: None,
            hover_pos: None,
            select_filter: Default::default(),
            picks: Vec::new(),
            pick_point: None,
            hover_pick: None,
            hover_body: None,
            pick_info: Default::default(),
            reveal_features: false,
            copied_placement: None,
            section_selected: None,
            section_pending: None,
            hidden: BTreeSet::new(),
            gpu: Gpu::new(rs.device.clone(), rs.queue.clone()),
            target: None,
            texture: None,
            list: None,
            caps: None,
            cap_plane: None,
            cut_indices: Default::default(),
            section_drag: None,
            section_basis: None,
            section_pivot: None,
            section_anchor: None,
            dirty: true,
            mesh_dirty: true,
            error: None,
            last_poll: Instant::now(),
            modified,
            pending: None,
            frame_ms: 0.,
            parameter_draft: None,
            saving: false,
            pending_pick_action: None,
            pick_tool_filter: None,
            feature_draft: None,
            sketch_draft: None,
            import_draft: None,
            placement_draft: None,
            move_draft: None,
            project_draft: None,
            #[cfg(test)]
            picked_file: None,
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
            window: Default::default(),
            search: Default::default(),
            marking: Default::default(),
            deleting: None,
            delete_notice: None,
        }
    }

    fn fit(&mut self) {
        let fitted = Camera::fit(&self.bodies);
        self.camera.target = fitted.target;
        self.camera.distance = fitted.distance;
        self.dirty = true;
    }

    fn view_controls(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .selectable_label(self.section_on, "Section")
                .on_hover_text("Section analysis (Shift+S)")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::ToggleSection);
            }
            if ui
                .add_enabled(
                    self.command_enabled(keys::Command::PressPull),
                    egui::Button::new("Press pull"),
                )
                .on_hover_text("Offset a planar face (Q)")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::PressPull);
            }
            ui.add_space(10.);
            if ui.button("Fit").on_hover_text("Fit view (F)").clicked() {
                self.run_command(ui.ctx(), keys::Command::Fit);
            }
            if ui.button("Top").on_hover_text("Top view").clicked() {
                self.run_command(ui.ctx(), keys::Command::Top);
            }
            if ui.button("Front").on_hover_text("Front view").clicked() {
                self.run_command(ui.ctx(), keys::Command::Front);
            }
            if ui
                .button("Iso")
                .on_hover_text("Fit the isometric view")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Iso);
            }
        });
    }

    fn project_panel(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(draft) = &mut self.project_draft else {
            return;
        };
        if !draft.create {
            self.project_draft = None;
            self.window.cancel();
            if self.dialog.is_none() {
                #[cfg(not(test))]
                {
                    self.dialog = Some(dialogs::open(frame, &self.root));
                }
                #[cfg(test)]
                {
                    let path = self.picked_file.take();
                    self.dialog = Some(dialogs::PendingDialog::new(async move {
                        dialogs::project_choice(path)
                    }));
                }
                ctx.request_repaint();
            }
            return;
        }
        let mut apply = false;
        let mut cancel = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::CreateProject,
            egui::Window::new("Create project").default_width(460.),
            ctx.content_rect(),
            |ui, _, can_apply| {
                ui.add_enabled_ui(!self.saving, |ui| {
                    let label = ui.label("Project folder");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut draft.path)
                            .labelled_by(label.id);
                        if ui
                            .add_enabled(self.dialog.is_none(), egui::Button::new("Browse"))
                            .clicked()
                        {
                            #[cfg(not(test))]
                            {
                                let picker = rfd::AsyncFileDialog::new().set_parent(frame);
                                self.dialog = Some(dialogs::PendingDialog::new(async move {
                                    dialogs::Choice::Folder(
                                        picker.pick_folder().await.map(|f| f.path().to_owned()),
                                    )
                                }));
                            }
                            #[cfg(test)]
                            {
                                let _ = frame;
                                let path = self.picked_file.take();
                                self.dialog = Some(dialogs::PendingDialog::new(async move {
                                    dialogs::Choice::Folder(path)
                                }));
                            }
                            ctx.request_repaint();
                        }
                    });
                    let label = ui.label("Project name");
                    ui.text_edit_singleline(&mut draft.name)
                        .labelled_by(label.id);
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                can_apply
                                    && self.pending.is_none()
                                    && !draft.path.trim().is_empty(),
                                egui::Button::new("Load project"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel project").clicked();
                    });
                });
                if self.saving {
                    ui.spinner();
                }
            },
        );
        cancel |= event == WindowEvent::Cancel;
        apply |=
            event == WindowEvent::Apply && self.pending.is_none() && !draft.path.trim().is_empty();
        if apply {
            let path = PathBuf::from(draft.path.trim());
            if !path.is_absolute() {
                self.window.error = Some("Use an absolute project folder path".into());
            } else {
                let name = Some(draft.name.trim().to_owned());
                self.start_project_load(ctx, path.clone(), path, name, false);
            }
        }
        if cancel {
            self.cancel_window();
        }
    }

    fn start_project_load(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        destination: PathBuf,
        name: Option<String>,
        archive: bool,
    ) {
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.pending_project = Some(destination.clone());
        self.saving = true;
        self.window.saving = self.window.open.is_some();
        self.error = None;
        std::thread::spawn(move || {
            let loaded = if archive {
                faber_wiring::archive::import(&path, &destination)
                    .and_then(|()| crate::edit::load_project(&destination, None))
            } else {
                crate::edit::load_project(&path, name)
            };
            let _ = tx.send(loaded);
            ctx.request_repaint();
        });
    }

    fn placement_panel(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.placement_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let group = !draft.others.is_empty();
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::MoveRotate,
            egui::Window::new("Move / rotate part").default_width(380.),
            ctx.content_rect(),
            |ui, _, can_apply| {
                if group {
                    ui.heading(format!("{} parts", draft.others.len() + 1));
                    ui.label(
                        "Relative move. Each value adds to every selected part, in mm and degrees.",
                    );
                    ui.label("The turn is about the common centre of the parts.");
                } else {
                    ui.heading(&draft.part);
                    ui.label("Position uses mm. Rotation uses degrees, X then Y then Z, about the part origin.");
                    ui.label("Values can use project parameters.");
                }
                ui.add_enabled_ui(!self.saving, |ui| {
                    egui::Grid::new("placement_fields").show(ui, |ui| {
                        let names = if group {
                            ("Move", "Turn")
                        } else {
                            ("Position", "Rotation")
                        };
                        for (prefix, values) in [
                            (names.0, &mut draft.translation),
                            (names.1, &mut draft.rotation),
                        ] {
                            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                                let resolved =
                                    faber_expr::resolve(&draft.expected.params).unwrap_or_default();
                                let unit = if prefix == names.0 { "mm" } else { "°" };
                                crate::feature_form::field(
                                    ui,
                                    &format!("{prefix} {axis}"),
                                    value,
                                    Some(unit),
                                    &resolved,
                                );
                                ui.end_row();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                can_apply && self.pending.is_none(),
                                egui::Button::new("Apply placement"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel placement").clicked();
                    });
                });
                if self.saving {
                    ui.spinner();
                }
            },
        );
        cancel |= event == WindowEvent::Cancel;
        apply |= event == WindowEvent::Apply && self.pending.is_none();
        if apply && group {
            let ids: Vec<String> = std::iter::once(draft.part.clone())
                .chain(draft.others.iter().cloned())
                .collect();
            let moves = match (
                move_tool::parse_vector(&draft.translation),
                move_tool::parse_vector(&draft.rotation),
            ) {
                (Some(translate), Some(turn)) => move_tool::relative_moves(
                    &draft.expected,
                    &self.bodies,
                    &ids,
                    translate,
                    turn.to_array(),
                ),
                _ => Err("Enter numbers in mm and degrees.".to_owned()),
            };
            match moves {
                Ok(moves) => {
                    let root = self.root.clone();
                    let expected = draft.expected.clone();
                    let strict = self.strict_edits;
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.pending = Some(rx);
                    self.saving = true;
                    self.window.saving = self.window.open.is_some();
                    self.error = None;
                    std::thread::spawn(move || {
                        let _ = tx.send(move_tool::commit(&root, &expected, moves, false, strict));
                        ctx.request_repaint();
                    });
                }
                Err(error) => self.window.error = Some(error),
            }
        } else if apply {
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
            self.window.saving = self.window.open.is_some();
            self.error = None;
            std::thread::spawn(move || {
                let _ = tx.send(crate::edit::place(
                    &root, &expected, &part, placement, strict,
                ));
                ctx.request_repaint();
            });
        }
        if cancel {
            self.cancel_window();
        }
    }

    fn import_panel(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.import_draft else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Import,
            egui::Window::new("Import model").default_width(460.),
            ctx.content_rect(),
            |ui, _, can_apply| {
                ui.label("Import a model. The project keeps a copy of the source file.");
                ui.label("STEP, STL, 3MF, OBJ, PLY or OFF. Mesh files keep their faces.");
                ui.add_enabled_ui(!self.saving, |ui| {
                    let label = ui.label("Source file");
                    ui.text_edit_singleline(&mut draft.path)
                        .labelled_by(label.id);
                    let label = ui.label("Part name");
                    ui.text_edit_singleline(&mut draft.name)
                        .labelled_by(label.id);
                    ui.checkbox(&mut draft.bought, "Purchased part");
                    if draft.bought {
                        for (title, value) in [
                            ("Source reference", &mut draft.source),
                            ("Licence", &mut draft.licence),
                            ("Designation", &mut draft.designation),
                        ] {
                            let label = ui.label(title);
                            ui.text_edit_singleline(value).labelled_by(label.id);
                        }
                    }
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                can_apply
                                    && self.pending.is_none()
                                    && !draft.path.trim().is_empty()
                                    && !draft.name.trim().is_empty(),
                                egui::Button::new("Import file"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel import").clicked();
                    });
                });
                if self.saving {
                    ui.spinner();
                }
            },
        );
        cancel |= event == WindowEvent::Cancel;
        apply |= event == WindowEvent::Apply
            && self.pending.is_none()
            && !draft.path.trim().is_empty()
            && !draft.name.trim().is_empty();
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
            let purchased = draft.bought.then(|| faber_model::Purchased {
                source: draft.source.trim().into(),
                licence: draft.licence.trim().into(),
                designation: draft.designation.trim().into(),
                mass_g: None,
            });
            let strict = self.strict_edits;
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.saving = true;
            self.window.saving = self.window.open.is_some();
            self.error = None;
            std::thread::spawn(move || {
                let _ = tx.send(crate::edit::import(
                    &root, &expected, &source, name, strict, purchased,
                ));
                ctx.request_repaint();
            });
        }
        if cancel {
            self.cancel_window();
        }
    }

    fn export_panel(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let mut cancel = false;
        let mut run = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Export,
            egui::Window::new("Export design").default_width(500.),
            ctx.content_rect(),
            |ui, _, can_apply| {
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
                run = ui.add_enabled(can_apply && self.pending.is_none() && !self.export_path.trim().is_empty(),egui::Button::new("Write files")).clicked();
            });
                if self.export_pending.is_some() {
                    ui.spinner();
                    ui.label("Checking and writing files…");
                }

                cancel = ui.button("Cancel").clicked();
            },
        );
        cancel |= event == WindowEvent::Cancel;
        run |= event == WindowEvent::Apply
            && self.pending.is_none()
            && self.export_pending.is_none()
            && !self.export_path.trim().is_empty();
        if cancel {
            self.cancel_window();
        }
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
            self.window.running = true;
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
        let mut cancel = false;
        let mut run = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Checks,
            egui::Window::new("Design checks").default_width(480.),
            ctx.content_rect(),
            |ui, _, can_apply| {
                ui.label("Mesh interference and clearance below 0.1 mm. Depth is approximate.");
                ui.checkbox(&mut self.strict_edits, "Refuse edits that add check errors");
                run = ui
                    .add_enabled(
                        self.check_pending.is_none() && can_apply && self.pending.is_none(),
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
                                                    self.extra_selected.clear();
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
                cancel = ui.button("Cancel").clicked();
            },
        );
        cancel |= event == WindowEvent::Cancel;
        run |=
            event == WindowEvent::Apply && self.pending.is_none() && self.check_pending.is_none();
        if cancel {
            self.cancel_window();
        }
        if run {
            let bodies = self.bodies.clone();
            let data = self.data.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.window.running = true;
            self.check_pending = Some(rx);
            std::thread::spawn(move || {
                let result = faber_wiring::interference(&bodies, 0.1, &data.contacts);
                let _ = tx.send((data, result));
                ctx.request_repaint();
            });
        }
    }

    fn move_history(&mut self, ctx: &egui::Context, redo: bool) {
        if self.pending.is_some() {
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
        let Some((expected, values)) = &mut self.parameter_draft else {
            return;
        };
        let parameter_count = values.len();
        let mut apply = false;
        let mut cancel = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Parameters,
            egui::Window::new("Edit parameters")
                .collapsible(false)
                .resizable(true)
                .default_width(420.),
            ctx.content_rect(),
            |ui, error, can_apply| {
                ui.label("Enter a value or an expression. Lengths use mm.");
                ui.add_enabled_ui(!self.saving, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.)
                        .show(ui, |ui| {
                            egui::Grid::new("parameter_editor").show(ui, |ui| {
                                let resolved = faber_expr::resolve(values).unwrap_or_default();
                                let mut remove = None;
                                for (name, expression) in values.iter_mut() {
                                    crate::feature_form::field(
                                        ui,
                                        name,
                                        expression,
                                        Some(""),
                                        &resolved,
                                    );
                                    if ui.button(format!("Remove {name}")).clicked() {
                                        remove = Some(name.clone());
                                    }
                                    ui.end_row();
                                }
                                if let Some(name) = remove {
                                    if let Some(owner) =
                                        crate::edit::parameter_user(expected, values, &name)
                                    {
                                        *error =
                                            Some(format!("Parameter {name} is used by {owner}."));
                                    } else {
                                        values.remove(&name);
                                        *error = None;
                                    }
                                }
                            });
                        });
                    egui::Grid::new("new_parameter").show(ui, |ui| {
                        for (label, text) in ["Parameter name", "Expression"]
                            .into_iter()
                            .zip(&mut self.new_parameter)
                        {
                            let response = ui.label(label);
                            ui.text_edit_singleline(text).labelled_by(response.id);
                            ui.end_row();
                        }
                    });
                    if ui
                        .add_enabled(
                            self.new_parameter.iter().all(|s| !s.trim().is_empty()),
                            egui::Button::new("Add parameter"),
                        )
                        .clicked()
                    {
                        let name = self.new_parameter[0].trim();
                        if values.contains_key(name) {
                            *error = Some(format!("Parameter {name} already exists."));
                        } else {
                            let mut next = values.clone();
                            next.insert(name.into(), self.new_parameter[1].trim().into());
                            match faber_expr::resolve(&next) {
                                Ok(_) => {
                                    *values = next;
                                    self.new_parameter = Default::default();
                                    *error = None;
                                }
                                Err(e) => *error = Some(e),
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                can_apply && self.pending.is_none(),
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
            },
        );
        self.window.dirty |= values.len() != parameter_count;
        cancel |= event == WindowEvent::Cancel;
        apply |= event == WindowEvent::Apply && self.pending.is_none();
        if cancel {
            self.cancel_window();
        }
        if apply && let Some((expected, values)) = self.parameter_draft.clone() {
            let root = self.root.clone();
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.saving = true;
            self.window.saving = self.window.open.is_some();
            self.error = None;
            let strict = self.strict_edits;
            std::thread::spawn(move || {
                let result = crate::edit::parameters(&root, &expected, values, strict);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }

    fn new_feature(&mut self, part_id: &str, kind: ToolKind) -> Result<(), String> {
        use faber_model::{Geometry, Scalar};
        let part = self.data.parts.get(part_id).ok_or("Select a part")?;
        faber_core::pick_selector::editable_part(part_id, part)?;
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
            ToolKind::Thread => "thread",
            ToolKind::Split => "split",
            ToolKind::Scale => "scale",
            ToolKind::Pipe => "pipe",
            ToolKind::PathPattern => "path_pattern",
            ToolKind::AnglePlane => "plane_angle",
            ToolKind::Midplane => "midplane",
            ToolKind::PointsPlane => "plane_points",
            ToolKind::CylinderAxis => "axis_cylinder",
            ToolKind::PlanesAxis => "axis_planes",
            ToolKind::CentrePoint => "point_centre",
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
                    cylinder_radius: None,
                    internal: None,
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
        } else if matches!(kind, ToolKind::AnglePlane) {
            let axes = reference_ids(part, is_axis);
            let axis = axes.first().ok_or("Add a reference axis first.")?.clone();
            Geometry::PlaneAtAngle {
                axis,
                from: faber_model::AngleFrom::Xy,
                angle: Scalar::Number(0.),
            }
        } else if matches!(kind, ToolKind::Midplane) {
            let planes = reference_ids(part, is_plane);
            if planes.len() < 2 {
                return Err("Add two reference planes first.".into());
            }
            Geometry::Midplane {
                first: planes[1].clone(),
                second: planes[0].clone(),
            }
        } else if matches!(kind, ToolKind::PointsPlane) {
            let at = |x: f64, y: f64| {
                faber_model::PointInput::Position([
                    Scalar::Number(x),
                    Scalar::Number(y),
                    Scalar::Number(0.),
                ])
            };
            Geometry::PlaneThroughPoints {
                points: [at(0., 0.), at(1., 0.), at(0., 1.)],
            }
        } else if matches!(kind, ToolKind::CylinderAxis) {
            Geometry::AxisFromCylinder {
                source,
                face: faber_model::FaceSelector {
                    cylinder_radius: Some(Scalar::Number(3.)),
                    internal: None,
                    created_by: None,
                    normal: None,
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: Scalar::Number(0.00001),
                    expect: 1,
                },
            }
        } else if matches!(kind, ToolKind::PlanesAxis) {
            let planes = reference_ids(part, is_plane);
            if planes.len() < 2 {
                return Err("Add two reference planes first.".into());
            }
            Geometry::AxisFromPlanes {
                first: planes[1].clone(),
                second: planes[0].clone(),
            }
        } else if matches!(kind, ToolKind::CentrePoint) {
            Geometry::PointAtCentre {
                source,
                edge: faber_model::EdgeSelector {
                    between: None,
                    of_face: None,
                    parallel: None,
                    near_centre: None,
                    in_box: None,
                    length: None,
                    radius: Some(Scalar::Number(3.)),
                    tolerance: Scalar::Number(0.00001),
                    expect: 1,
                },
            }
        } else if matches!(kind, ToolKind::Split) {
            Geometry::Split {
                source,
                origin: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(0.)],
                normal: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(1.)],
                keep: faber_model::SplitKeep::Both,
            }
        } else if matches!(kind, ToolKind::Thread) {
            Geometry::Thread {
                source,
                size: "M6".into(),
                length: Scalar::Number(3.),
                offset: Scalar::Number(0.),
                left_handed: false,
                face: faber_model::FaceSelector {
                    cylinder_radius: Some(Scalar::Number(3.)),
                    internal: Some(false),
                    created_by: None,
                    normal: None,
                    near_centre: None,
                    in_box: None,
                    area: None,
                    tolerance: Scalar::Number(0.00001),
                    expect: 1,
                },
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
                    cylinder_radius: None,
                    internal: None,
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
                    cylinder_radius: None,
                    internal: None,
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
                    radius_end: None,
                }
            } else {
                Geometry::Chamfer {
                    source,
                    edges,
                    distance: Scalar::Number(1.),
                    distance2: None,
                    angle_deg: None,
                    face: None,
                }
            }
        } else if matches!(kind, ToolKind::Shell) {
            Geometry::Shell {
                source,
                faces: faber_model::FaceSelector {
                    cylinder_radius: None,
                    internal: None,
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
        } else if matches!(
            kind,
            ToolKind::Scale | ToolKind::Pipe | ToolKind::PathPattern
        ) {
            feature_tools::new_geometry(&kind, source, &part.geometry)?
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
        self.request_window(Draft::Feature(form));
        self.error = None;
        Ok(())
    }

    fn feature_editor(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.feature_draft else {
            return;
        };
        let mut choose_pick = false;
        let mut preset_changed = false;
        let mut apply = false;
        let mut cancel = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Feature,
            egui::Window::new(if form.create {
                "New feature"
            } else {
                "Edit feature"
            })
            .default_width(460.),
            ctx.content_rect(),
            |ui, window_error, can_apply| {
                ui.heading(&form.id);
                ui.label("Lengths use mm. Angles use degrees.");
                if !form.help.is_empty() {
                    ui.label(form.help);
                }
                ui.add_enabled_ui(!self.saving, |ui| {
                    if let Some((label,_))=form.pick_field() {
                        choose_pick=ui.button(label).on_hover_text("Select items in the model view. Hold Ctrl to select more than one.").clicked();
                    }
                    match form.hole_options(ui) {
                        Ok(changed) => preset_changed = changed,
                        Err(error) => *window_error = Some(error),
                    }
                    form.split_options(ui);
                    form.path_options(ui);
                    if let Err(error) = form.option_fields(ui) {
                        *window_error = Some(error);
                    }
                    egui::ScrollArea::vertical()
                        .max_height(420.)
                        .show(ui, |ui| {
                            egui::Grid::new("feature_fields").show(ui, |ui| {
                                for (path, enabled) in &mut form.flags {
                                    ui.checkbox(enabled, crate::feature_form::label(path));
                                    ui.end_row();
                                }
                                let resolved =
                                    faber_expr::resolve(&form.expected.params).unwrap_or_default();
                                for (path, text, numeric) in &mut form.fields {
                                    let unit = crate::feature_form::field_unit(path, *numeric);
                                    crate::feature_form::field(
                                        ui,
                                        &crate::feature_form::label(path),
                                        text,
                                        unit,
                                        &resolved,
                                    );
                                    ui.end_row();
                                }
                            });
                        });
                    ui.horizontal(|ui| {
                        apply = ui
                            .add_enabled(
                                can_apply && self.pending.is_none() && self.pending_pick_action.is_none(),
                                egui::Button::new("Apply feature"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel feature").clicked();
                    });
                });
                if self.saving || self.pending_pick_action.is_some() {
                    ui.spinner();
                }
            },
        );
        self.window.dirty |= preset_changed;
        cancel |= event == WindowEvent::Cancel;
        apply |= event == WindowEvent::Apply
            && self.pending.is_none()
            && self.pending_pick_action.is_none();
        if apply {
            match form.geometry() {
                Err(error) => self.window.error = Some(error),
                Ok(geometry) => {
                    let root = self.root.clone();
                    let expected = form.expected.clone();
                    let part = form.part.clone();
                    let id = form.id.clone();
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.pending = Some(rx);
                    self.saving = true;
                    self.window.saving = self.window.open.is_some();
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
        if choose_pick {
            self.enter_pick_tool();
        }
        if cancel {
            self.cancel_window();
        }
    }

    fn sketch_editor(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.sketch_draft else {
            return;
        };
        let point_count = draft.points.len();
        let mut create = false;
        let mut cancel = false;
        let (event, _) = tool_window(
            ctx,
            &mut self.window,
            ToolWindow::Sketch,
            egui::Window::new("Create sketch")
                .default_width(640.)
                .collapsible(false),
            ctx.content_rect(),
            |ui, _, can_apply| {
                ui.add_enabled_ui(!self.saving, |ui| {
                    create = draft.ui(
                        ui,
                        can_apply && self.pending.is_none() && self.pending_pick_action.is_none(),
                    );
                    cancel = ui.button("Cancel sketch").clicked();
                });
                if self.saving || self.pending_pick_action.is_some() {
                    ui.spinner();
                }
            },
        );
        self.window.dirty |= draft.points.len() != point_count;
        cancel |= event == WindowEvent::Cancel;
        create |= event == WindowEvent::Apply
            && self.pending.is_none()
            && self.pending_pick_action.is_none();
        if create && self.pending.is_none() {
            match draft.part() {
                Err(error) => self.window.error = Some(error),
                Ok(part) => {
                    let root = self.root.clone();
                    let expected = draft.expected.clone();
                    let ctx = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.pending = Some(rx);
                    self.saving = true;
                    self.window.saving = self.window.open.is_some();
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
            self.cancel_window();
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
                Err(error) => {
                    if self.window.running && self.window.open == Some(ToolWindow::Export) {
                        self.window.error = Some(error.clone());
                    }
                    error
                }
            });
            self.export_pending = None;
        }
        if let Some(rx) = &self.check_pending
            && let Ok((data, result)) = rx.try_recv()
        {
            match result {
                Ok(report) => self.check_result = Some((data, report)),
                Err(error) => {
                    if self.window.running && self.window.open == Some(ToolWindow::Checks) {
                        self.window.error = Some(error.clone());
                    }
                    self.error = Some(error);
                }
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
            let deleted = self.deleting.take();
            match result {
                Ok((data, bodies)) => {
                    if let Some(names) = deleted {
                        self.delete_notice = Some((
                            format!("Deleted {names}. Ctrl+Z restores it."),
                            Instant::now(),
                        ));
                        ctx.request_repaint_after(Duration::from_secs(6));
                    }
                    let changing_project = self.pending_project.is_some();
                    if let Some(root) = self.pending_project.take() {
                        self.marking = Default::default();
                        self.project_loaded = true;
                        self.root = root;
                        self.modified = faber_wiring::project_fingerprint(&self.root).ok();
                        self.camera = Camera::fit(&bodies);
                        self.dirty = true;
                        self.select_only(None);
                        self.hidden.clear();
                        self.section_on = false;
                        self.section_selected = None;
                        self.section_pending = None;
                        self.check_result = None;
                        self.check_pending = None;
                        self.checks_open = false;
                        self.export_open = false;
                        self.export_path.clear();
                    }
                    if self.window.saving || changing_project {
                        self.cancel_window();
                    } else if data != self.data {
                        if self.window.dirty {
                            self.window.reload();
                        } else {
                            self.cancel_window();
                        }
                    }
                    if self.data.motion != data.motion || changing_project {
                        self.motion = Default::default();
                    }
                    if self.data != data || changing_project {
                        self.clear_entity_picks();
                        self.pick_info = Default::default();
                    }
                    self.data = data;
                    self.bodies = bodies.into();
                    self.list = None;
                    self.mesh_dirty = true;
                    self.error = None;
                    if changing_project {
                        self.remember_project();
                    }
                    self.sync_section(ctx);
                    self.hidden.retain(|id| self.data.parts.contains_key(id));
                    self.prune_selection();
                }
                Err(e) => {
                    if let Some(tool) = &mut self.press {
                        tool.submitting = false;
                    }
                    if self.window.saving {
                        self.window.error = Some(e.clone());
                    }
                    self.error = Some(e);
                }
            }
            self.pending = None;
            self.pending_project = None;
            self.saving = false;
            self.window.saving = false;
        }
        if self.project_loaded
            && self.pending.is_none()
            && self.last_poll.elapsed() > Duration::from_millis(500)
        {
            self.last_poll = Instant::now();
            #[cfg(debug_assertions)]
            match self.gpu.refresh_shader() {
                Ok(changed) => self.dirty |= changed,
                Err(message) => self.error = Some(message),
            }
            let modified = faber_wiring::project_fingerprint(&self.root).ok();
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
            && self.move_draft.is_none()
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

    fn placement_available(&self) -> bool {
        self.pending.is_none()
    }

    fn start_placement(&mut self, id: &str) {
        if !self.placement_available() {
            return;
        }
        let others = self.others_with(id);
        if let Some(part) = self.data.parts.get(id) {
            let text = |value: &faber_model::Scalar| match value {
                faber_model::Scalar::Number(number) => number.to_string(),
                faber_model::Scalar::Expression(expression) => expression.clone(),
            };
            // A group takes relative values, so the fields start at zero.
            let (translation, rotation) = if others.is_empty() {
                (
                    part.placement.translation.each_ref().map(text),
                    part.placement.rotation.each_ref().map(text),
                )
            } else {
                (zero_fields(), zero_fields())
            };
            self.request_window(Draft::Placement(PlacementDraft {
                expected: self.data.clone(),
                part: id.to_owned(),
                translation,
                rotation,
                others,
            }));
            self.error = None;
        }
    }

    fn body_menu(&mut self, ui: &mut egui::Ui) {
        // Hide, isolate and delete act on the whole selection.
        let ids = self.selection();
        if let Some(id) = self.selected.clone() {
            if ui
                .add_enabled(
                    self.placement_available(),
                    egui::Button::new("Move/Copy").shortcut_text("M"),
                )
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Move);
                ui.close();
            }
            if ui
                .add_enabled(
                    self.placement_available(),
                    egui::Button::new("Point to point…"),
                )
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::PointToPoint);
                ui.close();
            }
            if ui
                .add_enabled(self.placement_available(), egui::Button::new("Align to…"))
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Align);
                ui.close();
            }
            if ui
                .add_enabled(
                    matches!(
                        self.data.parts[&id].geometry,
                        faber_model::Geometry::History { .. }
                    ),
                    egui::Button::new("Edit features"),
                )
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::EditFeatures);
                ui.close();
            }
            if ui.button("Copy placement").clicked() {
                self.run_command(ui.ctx(), keys::Command::CopyPlacement);
                ui.close();
            }
            let hidden = self.hidden.contains(&id);
            let plural = if ids.len() > 1 { "parts" } else { "body" };
            if ui
                .button(format!("{} {plural}", if hidden { "Show" } else { "Hide" }))
                .clicked()
            {
                for part in &ids {
                    if hidden {
                        self.hidden.remove(part);
                    } else {
                        self.hidden.insert(part.clone());
                    }
                }
                self.mesh_dirty = true;
                ui.close();
            }
            if ui.button(format!("Isolate {plural}")).clicked() {
                self.run_command(ui.ctx(), keys::Command::Isolate);
                ui.close();
            }
            let delete = if ids.len() > 1 {
                "Delete parts"
            } else {
                "Delete part"
            };
            if ui
                .add_enabled(self.placement_available(), egui::Button::new(delete))
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Delete);
                ui.close();
            }
            ui.separator();
        }
        if self.selected.is_none() {
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("New sketch"))
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Sketch);
                ui.close();
            }
            if ui
                .selectable_label(self.section_on, "Section analysis")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::ToggleSection);
                ui.close();
            }
        }
        if self.copied_placement.is_some()
            && ui
                .add_enabled(
                    self.command_enabled(keys::Command::PastePlacement),
                    egui::Button::new("Paste placement"),
                )
                .clicked()
        {
            self.run_command(ui.ctx(), keys::Command::PastePlacement);
            ui.close();
        }
        if ui
            .add_enabled(
                !self.hidden.is_empty(),
                egui::Button::new("Show all bodies"),
            )
            .clicked()
        {
            self.run_command(ui.ctx(), keys::Command::ShowAll);
            ui.close();
        }
        if ui.button("Fit view").clicked() {
            self.run_command(ui.ctx(), keys::Command::Fit);
            ui.close();
        }
    }

    fn tree(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, create) in [("New project", true), ("Open project", false)] {
                if ui
                    .add_enabled(self.pending.is_none(), egui::Button::new(label))
                    .clicked()
                {
                    self.request_window(Draft::Project(ProjectDraft {
                        create,
                        path: String::new(),
                        name: "Untitled".into(),
                    }));
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
                "{} {}",
                self.bodies.len(),
                if self.bodies.len() == 1 {
                    "body"
                } else {
                    "bodies"
                }
            ))
            .color(MUTED),
        );
        ui.add_space(20.);
        ui.separator();
        ui.add_space(12.);
        ui.label(RichText::new("BODIES").size(11.).color(MUTED));
        ui.add_space(10.);
        let chosen = self.selection();
        for body in std::sync::Arc::clone(&self.bodies).iter() {
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
                let selected = chosen.contains(&body.id);
                let row = ui
                    .add(egui::Button::selectable(selected, &body.name).truncate())
                    .on_hover_text(&body.name);
                if row.clicked() || row.secondary_clicked() {
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
        ui.add_space(24.);
        if ui.button("Show all").clicked() {
            self.hidden.clear();
            self.mesh_dirty = true;
        }
        let part_id = self.selected.clone();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    part_id.is_some() && self.placement_available(),
                    egui::Button::new("Move/Copy"),
                )
                .on_disabled_hover_text("Select a part first.")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::Move);
            }
            if ui
                .add_enabled(
                    part_id.is_some() && self.placement_available(),
                    egui::Button::new("Exact placement"),
                )
                .on_disabled_hover_text("Select a part first.")
                .clicked()
            {
                self.run_command(ui.ctx(), keys::Command::ExactPlacement);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("Linear pattern", ToolKind::Linear),
                ("Circular pattern", ToolKind::Circular),
                ("Mirror", ToolKind::Mirror),
                ("Shell", ToolKind::Shell),
                ("Scale", ToolKind::Scale),
                ("Pipe", ToolKind::Pipe),
                ("Path pattern", ToolKind::PathPattern),
                ("Fillet", ToolKind::Fillet),
                ("Chamfer", ToolKind::Chamfer),
                ("Offset solid", ToolKind::Offset),
                ("Draft", ToolKind::Draft),
                ("Thicken face", ToolKind::Thicken),
                ("Hole", ToolKind::Hole),
                ("Thread", ToolKind::Thread),
                ("Split body", ToolKind::Split),
            ] {
                if ui
                    .add_enabled(
                        part_id.is_some() && self.pending.is_none(),
                        egui::Button::new(label),
                    )
                    .on_disabled_hover_text("Select a part first.")
                    .clicked()
                {
                    self.run_command(ui.ctx(), keys::Command::Feature(kind));
                }
            }
            ui.menu_button("References", |ui| {
                for (label, kind) in [
                    ("Reference plane", ToolKind::Plane),
                    ("Plane on face", ToolKind::FacePlane),
                    ("Reference axis", ToolKind::Axis),
                    ("Reference point", ToolKind::Point),
                    ("Plane at angle", ToolKind::AnglePlane),
                    ("Midplane", ToolKind::Midplane),
                    ("Plane through points", ToolKind::PointsPlane),
                    ("Axis from cylinder", ToolKind::CylinderAxis),
                    ("Axis from planes", ToolKind::PlanesAxis),
                    ("Point at centre", ToolKind::CentrePoint),
                ] {
                    if ui
                        .add_enabled(
                            part_id.is_some() && self.pending.is_none(),
                            egui::Button::new(label),
                        )
                        .on_disabled_hover_text("Select a part first.")
                        .clicked()
                    {
                        self.run_command(ui.ctx(), keys::Command::Feature(kind));
                        ui.close();
                    }
                }
            });
        });
        let part = self
            .selected
            .as_ref()
            .and_then(|id| self.data.parts.get(id));
        if let Some(part) = part
            && let faber_model::Geometry::History { features, output } = &part.geometry
        {
            ui.add_space(20.);
            ui.separator();
            ui.add_space(12.);
            let heading = ui.label(RichText::new("FEATURES").size(11.).color(MUTED));
            if std::mem::take(&mut self.reveal_features) {
                heading.scroll_to_me(Some(egui::Align::Min));
            }
            let mut next_feature = None;
            let mut suppress: Option<(String, bool)> = None;
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
                                    self.pending.is_none(),
                                    egui::Button::new(if feature.suppressed {
                                        RichText::new(&feature.id).color(MUTED).strikethrough()
                                    } else {
                                        RichText::new(&feature.id).color(if &feature.id == output {
                                            ACCENT
                                        } else {
                                            Color32::LIGHT_GRAY
                                        })
                                    }),
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
                                    Ok(form) => next_feature = Some(form),
                                    Err(error) => self.error = Some(error),
                                }
                            }
                            if ui
                                .add_enabled(
                                    self.pending.is_none()
                                        && self.feature_draft.is_none()
                                        && self.parameter_draft.is_none()
                                        && self.sketch_draft.is_none()
                                        && self.import_draft.is_none()
                                        && self.placement_draft.is_none()
                                        && self.move_draft.is_none()
                                        && self.project_draft.is_none()
                                        && &feature.id != output,
                                    egui::Button::new(if feature.suppressed {
                                        "Unsuppress"
                                    } else {
                                        "Suppress"
                                    }),
                                )
                                .clicked()
                            {
                                suppress = Some((feature.id.clone(), !feature.suppressed));
                            }
                        });
                    }
                });
            if let Some(form) = next_feature {
                self.request_window(Draft::Feature(form));
            }
            if let Some((id, suppressed)) = suppress
                && let Some(part) = &part_id
            {
                self.save_sections(
                    ui.ctx(),
                    vec![faber_model::Edit::SetSuppressed {
                        part: part.clone(),
                        id,
                        suppressed,
                        cascade: false,
                    }],
                );
            }
        }
        {
            ui.add_space(20.);
            ui.separator();
            ui.add_space(12.);
            ui.label(RichText::new("PARAMETERS").size(11.).color(MUTED));
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Edit parameters"))
                .clicked()
            {
                self.request_window(Draft::Parameters(
                    self.data.clone(),
                    self.data.params.clone(),
                ));
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
        if self.study_panel(ui) {
            return;
        }
        self.pick_inspector(ui);
        self.motion_controls(ui);
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
                    let volume_label = if body.shape.is_some() {
                        "Solid volume"
                    } else {
                        "Volume estimate"
                    };
                    ui.label(RichText::new(volume_label).color(MUTED));
                    ui.label(format!("{:.1} cm³", body.volume / 1000.));
                    ui.end_row();
                });
            ui.add_space(16.);
            if let Some(part) = self.data.parts.get(&body.id)
                && !part.note.is_empty()
            {
                ui.label(RichText::new(&part.note).small().color(MUTED));
            }
            if self.selected.as_ref() == Some(&body.id)
                && let Some(choice) =
                    material::selector(ui, &self.data, body, self.pending.is_none())
            {
                self.save_material(ui.ctx(), body.id.clone(), choice);
            }
        }
        if crate::overhang::panel(ui, &mut self.overhang) {
            self.list = None;
            self.dirty = true;
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
            for b in self.bodies.iter() {
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
            if ui.checkbox(&mut self.gpu.hatch, "Show hatching").changed() {
                self.dirty = true;
            }
            let mut custom = self.gpu.cut_colour.is_some();
            ui.horizontal(|ui| {
                if ui.checkbox(&mut custom, "Custom cut colour").changed() {
                    self.gpu.cut_colour = custom.then_some([228, 174, 205]);
                    self.dirty = true;
                }
                if let Some(colour) = &mut self.gpu.cut_colour
                    && ui.color_edit_button_srgb(colour).changed()
                {
                    self.dirty = true;
                }
            });
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
                RichText::new("F  Fit view     S  Commands     Shift+S  Section")
                    .small()
                    .color(MUTED),
            );
            ui.label(
                RichText::new(
                    "Drag empty space to select. Middle drag to pan. Right drag for commands.\nShift + middle drag or Alt + left drag to orbit. Scroll to zoom.",
                )
                    .small()
                    .color(MUTED),
            );

        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
        let (id, rect) = ui.allocate_space(ui.available_size());
        let hit = rect.shrink2(Vec2::new(5., 0.));
        let response = ui.interact(hit, id, egui::Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), "Model viewport")
        });
        if response.hovered()
            && let Some(pos) = response.hover_pos()
        {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                let factor = (-scroll * 0.002).exp();
                if let Some((_, point)) = self.pick_surface(rect, pos) {
                    self.camera.zoom_about(point.as_vec3(), factor);
                } else {
                    let ndc = [
                        2. * (pos.x - rect.left()) / rect.width() - 1.,
                        1. - 2. * (pos.y - rect.top()) / rect.height(),
                    ];
                    self.camera.zoom_at(rect.aspect_ratio(), ndc, factor);
                }
                self.dirty = true;
            }
        }
        if response.double_clicked_by(egui::PointerButton::Middle) {
            self.fit();
        }
        if !self.section_on {
            self.section_pivot = None;
        }
        let handle = self.section_on.then(|| {
            let displacement = ui.input(|i| {
                i.pointer
                    .interact_pos()
                    .zip(i.pointer.press_origin())
                    .map(|(p, start)| p - start)
            });
            if let Some(displacement) = displacement
                && let Some(SectionDrag::Move {
                    axis,
                    scale,
                    offset,
                }) = self.section_drag
            {
                self.plane.offset = offset + (displacement.dot(axis) / scale) as f64;
            }
            if let Some(displacement) = displacement
                && let Some(SectionDrag::Rotate {
                    axis,
                    normal,
                    pivot,
                    rate,
                    basis,
                    ..
                }) = self.section_drag
            {
                let turn = glam::Quat::from_axis_angle(axis, displacement.dot(rate));
                let normal = (turn * normal).normalize();
                self.section_basis = Some(turn * basis);
                self.plane.normal = normal.as_dvec3().to_array();
                self.plane.offset = pivot.as_dvec3().dot(normal.as_dvec3());
            }
            if self.section_drag.is_some() {
                self.section_selected = None;
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                self.section_drag = None;
            }
            let n = glam::Vec3::from(self.plane.normal.map(|x| x as f32)).normalize();
            let offset = self.plane.offset as f32;
            let origin = self.section_pivot.unwrap_or_else(|| {
                let mut low = glam::Vec3::splat(f32::INFINITY);
                let mut high = -low;
                let mut model_low = low;
                let mut model_high = high;
                for body in self
                    .bodies
                    .iter()
                    .filter(|body| !self.hidden.contains(&body.id))
                {
                    let pose = self
                        .motion
                        .poses
                        .get(&body.id)
                        .copied()
                        .unwrap_or(glam::Mat4::IDENTITY);
                    let corners: [glam::Vec3; 8] = std::array::from_fn(|mask| {
                        pose.transform_point3(glam::Vec3::from_array(std::array::from_fn(|i| {
                            body.bounds[(mask >> i) & 1][i] as f32
                        })))
                    });
                    for (mask, &start) in corners.iter().enumerate() {
                        model_low = model_low.min(start);
                        model_high = model_high.max(start);
                        for bit in [1, 2, 4] {
                            if mask & bit != 0 {
                                continue;
                            }
                            let end = corners[mask | bit];
                            let d = n.dot(start) - offset;
                            let e = n.dot(end) - offset;
                            if d.min(e) <= 0. && d.max(e) >= 0. {
                                let hit = if d == e {
                                    (start + end) * 0.5
                                } else {
                                    start.lerp(end, d / (d - e))
                                };
                                low = low.min(hit);
                                high = high.max(hit);
                            }
                        }
                    }
                }
                if low.is_finite() {
                    (low + high) * 0.5
                } else if model_low.is_finite() {
                    (model_low + model_high) * 0.5
                } else {
                    glam::Vec3::ZERO
                }
            });
            let mut anchor = origin + n * (offset - n.dot(origin));
            let matrix = self.camera.matrix(rect.aspect_ratio());
            // Keep the handle in the view. A close view of a large model puts the
            // model anchor off the screen. Then use the plane point nearest to it
            // that the view shows. A drag keeps its own pivot.
            if self.section_drag.is_none() {
                let seen = matrix.project_point3(anchor);
                let at =
                    rect.center() + Vec2::new(seen.x * rect.width(), -seen.y * rect.height()) * 0.5;
                let inside = rect.shrink(90f32.min(rect.size().min_elem() * 0.4));
                let in_depth = (0.0..=1.0).contains(&seen.z);
                let outside = !in_depth || !inside.contains(at);
                let cut = outside
                    .then(|| {
                        let caps = self
                            .caps
                            .as_ref()
                            .filter(|_| self.cap_plane == Some(self.plane));
                        visible_anchor(
                            cut_centroids(caps, &self.bodies, &self.hidden).into_iter(),
                            matrix,
                            rect,
                            inside,
                        )
                    })
                    .flatten();
                if let Some(point) = cut {
                    anchor = point;
                } else if outside {
                    let target = if in_depth {
                        inside.clamp(at)
                    } else {
                        rect.center()
                    };
                    let ndc = (target - rect.center()) / (rect.size() * 0.5);
                    let inverse = matrix.inverse();
                    let near = inverse.project_point3(glam::Vec3::new(ndc.x, -ndc.y, 0.));
                    let far = inverse.project_point3(glam::Vec3::new(ndc.x, -ndc.y, 1.));
                    let ray = far - near;
                    let along = n.dot(ray);
                    if along.abs() > 1e-6 {
                        let t = (offset - n.dot(near)) / along;
                        if (0.0..=1.0).contains(&t) {
                            anchor = near + ray * t;
                        }
                    }
                }
            }
            self.section_anchor = Some(anchor);
            let screen = |world| {
                let p = matrix.project_point3(world);
                rect.center() + Vec2::new(p.x * rect.width(), -p.y * rect.height()) * 0.5
            };
            let centre = screen(anchor);
            let projected = screen(anchor + n) - centre;
            let (axis, scale) = if projected.length() > 0.1 {
                (projected.normalized(), projected.length())
            } else {
                (-Vec2::Y, rect.height() / self.camera.distance)
            };
            let fine = ui.input(|i| i.modifiers.shift);
            let gain = if fine { 0.05 } else { 0.35 };
            let screen_scale = rect.height() / self.camera.distance;
            let tip = centre + axis * 58.;
            let basis = self
                .section_basis
                .and_then(|b| (b - n * b.dot(n)).try_normalize())
                .unwrap_or_else(|| n.any_orthonormal_vector());
            let directions = [basis, n.cross(basis)];
            let world_scale = self.camera.distance / rect.height() * 64.;
            let arc_point = |index: usize, t: f32| {
                let offset = n * t.cos() + directions[index] * t.sin();
                centre + (screen(anchor + offset * world_scale) - screen(anchor))
            };
            let arcs: [Vec<egui::Pos2>; 2] = std::array::from_fn(|i| {
                (0..=44)
                    .map(|j| arc_point(i, -1.45 + j as f32 * 2.9 / 44.))
                    .collect()
            });
            let grips = [arc_point(0, 1.1), arc_point(1, -1.1)];
            let mut lo = [f32::INFINITY; 2];
            let mut hi = [f32::NEG_INFINITY; 2];
            for body in self.bodies.iter().filter(|b| !self.hidden.contains(&b.id)) {
                let pose = self
                    .motion
                    .poses
                    .get(&body.id)
                    .copied()
                    .unwrap_or(glam::Mat4::IDENTITY);
                for mask in 0..8 {
                    let point = glam::Vec3::from_array(std::array::from_fn(|i| {
                        body.bounds[(mask >> i) & 1][i] as f32
                    }));
                    for i in 0..2 {
                        let d = (pose.transform_point3(point) - anchor).dot(directions[i]);
                        lo[i] = lo[i].min(d);
                        hi[i] = hi[i].max(d);
                    }
                }
            }
            let quad = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(x, y)| {
                let margin = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.04).max(1.);
                let limits = [
                    [lo[0] - margin, lo[1] - margin],
                    [hi[0] + margin, hi[1] + margin],
                ];
                matrix
                    * (anchor + directions[0] * limits[x][0] + directions[1] * limits[y][1])
                        .extend(1.)
            });

            // Clip at the near plane before the perspective divide.
            let mut corners = Vec::with_capacity(5);
            let mut previous = quad[3];
            for current in quad {
                let project = |p: glam::Vec4| {
                    rect.center()
                        + Vec2::new(p.x * rect.width(), -p.y * rect.height()) * (0.5 / p.w)
                };
                if (previous.z >= 0.) != (current.z >= 0.) {
                    corners.push(project(
                        previous.lerp(current, previous.z / (previous.z - current.z)),
                    ));
                }
                if current.z >= 0. {
                    corners.push(project(current));
                }
                previous = current;
            }
            let control = |p, id| {
                ui.interact(
                    egui::Rect::from_center_size(p, Vec2::splat(38.)),
                    egui::Id::unique(id),
                    Sense::drag(),
                )
            };
            let movement = control(tip, "section_offset_handle").on_hover_text(
                "Drag to move the section. Hold Shift before dragging for fine control.",
            );
            let rotation = [
                control(grips[0], "section_tilt_handle")
                    .on_hover_text("Drag to tilt. Hold Shift before dragging for fine control."),
                control(grips[1], "section_turn_handle")
                    .on_hover_text("Drag to turn. Hold Shift before dragging for fine control."),
            ];
            let segment_distance = |p: egui::Pos2, a: egui::Pos2, b: egui::Pos2| {
                let line = b - a;
                let t = ((p - a).dot(line) / line.length_sq().max(0.0001)).clamp(0., 1.);
                p.distance(a + line * t)
            };
            let nearest_stroke = |p: egui::Pos2| {
                let arc_distance = |arc: &[egui::Pos2]| {
                    arc.windows(2)
                        .map(|v| segment_distance(p, v[0], v[1]))
                        .fold(f32::INFINITY, f32::min)
                };
                [
                    segment_distance(p, centre, tip),
                    arc_distance(&arcs[0]),
                    arc_distance(&arcs[1]),
                ]
                .into_iter()
                .enumerate()
                .filter(|(_, d)| *d <= 7.)
                .min_by(|(_, a), (_, b)| a.total_cmp(b))
                .map(|(i, _)| i)
            };
            if response.hovered()
                && ui
                    .ctx()
                    .pointer_hover_pos()
                    .and_then(nearest_stroke)
                    .is_some()
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if ui.input(|i| i.pointer.primary_pressed()) {
                self.section_drag = ui.input(|i| i.pointer.press_origin()).and_then(|p| {
                    let nearest = [tip, grips[0], grips[1]]
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _)| {
                            if *i == 0 {
                                movement.rect.contains(p)
                            } else {
                                rotation[*i - 1].rect.contains(p)
                            }
                        })
                        .min_by(|(_, a), (_, b)| a.distance_sq(p).total_cmp(&b.distance_sq(p)))
                        .map(|(i, _)| i)
                        .or_else(|| nearest_stroke(p));
                    if nearest == Some(0) {
                        Some(SectionDrag::Move {
                            axis,
                            scale: scale.max(screen_scale).max(0.5) / gain,
                            offset: self.plane.offset,
                        })
                    } else if let Some(i) = nearest.map(|index| index - 1) {
                        let segment = arcs[i].windows(2).min_by(|a, b| {
                            segment_distance(p, a[0], a[1])
                                .total_cmp(&segment_distance(p, b[0], b[1]))
                        })?;
                        Some(SectionDrag::Rotate {
                            index: i,
                            axis: n.cross(directions[i]).normalize(),
                            normal: n,
                            pivot: anchor,
                            rate: (segment[1] - segment[0]).normalized() * (0.014 * gain),
                            basis,
                        })
                    } else {
                        None
                    }
                });
                if self.section_drag.is_some() {
                    self.section_pivot = Some(anchor);
                }
            }
            (
                centre,
                tip,
                grips,
                self.section_drag.is_some(),
                arcs,
                corners,
            )
        });
        let moving = self.press_interact(ui, rect) || self.move_canvas(ui, rect);
        let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
        let busy = moving
            || alt
            || self.marking.anchor.is_some()
            || self.move_draft.is_some()
            || handle.as_ref().is_some_and(|h| h.3);
        let boxed = self.select_filter == SelectFilter::Body
            && self.box_select(ui, rect, busy, response.hovered());
        let delta = ui.input(|i| i.pointer.delta());
        let joint_drag = self.drag_joint(
            ui,
            rect,
            &response,
            handle.as_ref().is_some_and(|h| h.3) || moving,
        );
        // Fusion mapping. Left drag on empty space selects. Shift + middle, or Alt + left, orbits.
        // Middle drag pans. Right drag selects a menu command.
        let orbit = (response.dragged_by(egui::PointerButton::Middle) && shift)
            || (response.dragged_by(egui::PointerButton::Primary) && alt);
        if orbit && !handle.as_ref().is_some_and(|h| h.3) && !joint_drag && !moving && !boxed {
            self.camera.yaw -= delta.x * 0.35;
            self.camera.pitch = (self.camera.pitch + delta.y * 0.35).clamp(-85., 89.);
            self.dirty = true;
        }
        let pan = response.dragged_by(egui::PointerButton::Middle) && !shift;
        if pan {
            let (right, up) = self.camera.screen_axes();
            self.camera.target +=
                (-right * delta.x + up * delta.y) * self.camera.distance / rect.height();
            self.dirty = true;
        }
        let right_press = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
        if right_press
            && self.marking.anchor.is_none()
            && response.contains_pointer()
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            self.start_mark(rect, pos);
        }
        if response.clicked()
            && self.marking.anchor.is_none()
            && !moving
            && !boxed
            && let Some(pos) = response.interact_pointer_pos()
        {
            let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            self.pick_canvas(rect, pos, ctrl, shift, response.double_clicked(), false);
        }
        let hover = if busy || boxed || ui.input(|i| i.pointer.any_down()) {
            None
        } else {
            response.hover_pos().filter(|_| response.hovered())
        };
        if self.hover_pos != hover || (hover.is_some() && (self.dirty || self.mesh_dirty)) {
            self.hover_entity(rect, hover);
        }
        self.marking_menu(ui.ctx());
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
        let study_paint = self.study.paint.as_ref().filter(|_| self.press.is_none());
        if study_paint.is_some() {
            self.gpu.hovered = None;
        }
        let display_bodies = self
            .press
            .as_ref()
            .and_then(|tool| tool.preview.as_ref())
            .map(|b| b.as_slice())
            .or_else(|| study_paint.map(|p| p.bodies.as_slice()))
            .unwrap_or(&self.bodies);
        if self.list.is_none() {
            self.cut_indices = std::sync::Arc::new(
                display_bodies
                    .iter()
                    .map(|b| faber_scene::SectionIndex::new(&b.mesh))
                    .collect(),
            );
            let uploaded = if let Some(p) = study_paint {
                self.gpu.upload_stress(display_bodies, &p.part, &p.ratios)
            } else {
                let paint = self.overhang.paint(display_bodies);
                self.gpu
                    .upload_painted(display_bodies, &BTreeSet::new(), None, None, &paint)
            };
            match uploaded {
                Ok(list) => self.list = Some(list),
                Err(e) => self.error = Some(e),
            }
            self.cap_plane = None;
            self.caps = None;
            self.dirty = true;
        }
        let plane = self.section_on.then_some(self.plane);
        if plane != self.cap_plane {
            self.caps = None;
            if let Some(next) = plane {
                match Gpu::caps(
                    &self.gpu.device,
                    display_bodies,
                    &self.cut_indices,
                    &BTreeSet::new(),
                    next,
                    &self.motion.poses,
                ) {
                    Ok(caps) => self.caps = caps,
                    Err(error) => self.error = Some(error),
                }
            }
            self.cap_plane = plane;
            self.dirty = true;
        }
        self.dirty |= self.mesh_dirty;
        self.mesh_dirty = false;
        if self.dirty
            && let Some(list) = &mut self.list
        {
            list.mark_faces(
                &self.gpu.queue,
                self.picks
                    .iter()
                    .filter(|_| study_paint.is_none())
                    .filter_map(Pick::face_key),
                self.hover_pick
                    .as_ref()
                    .filter(|_| study_paint.is_none())
                    .and_then(Pick::face_key),
            );
        }
        if self.dirty
            && let (Some(target), Some(list)) = (&self.target, &self.list)
        {
            let start = Instant::now();
            self.gpu.draw_visible_view(
                target,
                list,
                self.camera,
                self.selected
                    .as_deref()
                    .filter(|_| study_paint.is_none())
                    .filter(|_| {
                        self.select_filter == SelectFilter::Body
                            || matches!(self.picks.last(), Some(Pick::Body(_)))
                    }),
                self.cap_plane,
                self.caps.as_ref(),
                &self.move_poses(),
                &self.hidden,
            );
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
        if study_paint.is_none() {
            self.draw_pick_edges(ui, rect);
        }
        self.study_markers(ui, rect);
        self.view_cube(ui, rect);
        let painter = ui.painter();
        if let Some((centre, tip, grips, _, arcs, corners)) = handle {
            let painter = painter.with_clip_rect(rect);
            if corners.len() >= 3 && corners.iter().all(|p| p.is_finite()) {
                painter.add(egui::Shape::convex_polygon(
                    corners,
                    Color32::from_rgba_unmultiplied(248, 202, 118, 16),
                    egui::Stroke::new(1., Color32::from_rgba_unmultiplied(102, 82, 46, 65)),
                ));
            }
            for (i, arc) in arcs.into_iter().enumerate() {
                let active = matches!(self.section_drag, Some(SectionDrag::Rotate { index, .. }) if index == i);
                let hovered = ui
                    .ctx()
                    .pointer_hover_pos()
                    .is_some_and(|p| p.distance(grips[i]) <= 19.);
                let ink = if active || hovered {
                    Color32::from_rgb(0, 115, 180)
                } else {
                    Color32::from_rgba_unmultiplied(65, 70, 75, 85)
                };
                painter.add(egui::Shape::line(arc, egui::Stroke::new(2.5, ink)));
                painter.circle(
                    grips[i],
                    11.,
                    Color32::from_white_alpha(if active || hovered { 235 } else { 105 }),
                    egui::Stroke::new(1.6, ink),
                );
                painter.circle_filled(grips[i], 3., ink);
            }
            painter.arrow(
                centre,
                tip - centre,
                egui::Stroke::new(
                    2.5,
                    Color32::from_rgba_unmultiplied(
                        0,
                        139,
                        199,
                        if self.section_drag.is_some()
                            || ui
                                .ctx()
                                .pointer_hover_pos()
                                .is_some_and(|p| p.distance(tip) <= 19.)
                        {
                            255
                        } else {
                            145
                        },
                    ),
                ),
            );
            painter.circle(
                centre,
                3.5,
                Color32::WHITE,
                egui::Stroke::new(1., Color32::DARK_GRAY),
            );
        }
        self.press_paint(ui, rect);
        self.move_paint(ui, rect);
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
        self.poll_dialog(&ctx);
        self.poll(&ctx);
        self.poll_study(&ctx);
        self.poll_press();
        self.shortcut(&ctx);
        if !self.project_loaded {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(
                        (ui.available_height() * if self.recent.is_empty() { 0.22 } else { 0.06 })
                            .max(12.),
                    );
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
                                    self.request_window(Draft::Project(ProjectDraft {
                                        create,
                                        path: String::new(),
                                        name: "Untitled".into(),
                                    }));
                                    self.error = None;
                                }
                                ui.add_space(12.);
                            }
                        },
                    );
                    if !self.recent.is_empty() {
                        ui.add_space(8.);
                        ui.label(RichText::new("Recent projects").strong());
                        ui.add_space(6.);
                        let mut open = None;
                        egui::ScrollArea::vertical()
                            .max_height((ui.available_height() - 8.).max(40.))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    for (name, path) in &self.recent {
                                        let row = ui.add_enabled(
                                            self.pending.is_none() && self.project_draft.is_none(),
                                            egui::Button::new(name.as_str())
                                                .min_size(Vec2::new(260., 32.)),
                                        );
                                        if row.on_hover_text(path.display().to_string()).clicked() {
                                            open = Some(path.clone());
                                        }
                                    }
                                });
                            });
                        if let Some(path) = open {
                            self.start_project_load(&ctx, path.clone(), path, None, false);
                        }
                    }
                    if self.project_draft.is_none()
                        && let Some(error) = &self.error
                    {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                });
            });
            self.project_panel(&ctx, frame);
            self.discard_question(&ctx);
            return;
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
                    if self.strict_edits
                        && ui
                            .selectable_label(true, "Strict")
                            .on_hover_text("Turn off strict edits")
                            .clicked()
                    {
                        self.run_command(&ctx, keys::Command::ToggleStrict);
                    }
                    if ui
                        .button("Checks")
                        .on_hover_text("Inspect the design")
                        .clicked()
                    {
                        self.run_command(&ctx, keys::Command::Checks);
                    }
                    if ui
                        .button("Export")
                        .on_hover_text("Export the design")
                        .clicked()
                    {
                        self.run_command(&ctx, keys::Command::Export);
                    }
                    if ui
                        .add_enabled(self.pending.is_none(), egui::Button::new("Import model"))
                        .on_hover_text("Add an existing model")
                        .clicked()
                    {
                        self.run_command(&ctx, keys::Command::Import);
                        self.error = None;
                    }
                    if ui
                        .add_enabled(self.pending.is_none(), egui::Button::new("New sketch"))
                        .on_hover_text("Draw a profile")
                        .clicked()
                    {
                        self.run_command(&ctx, keys::Command::Sketch);
                        self.error = None;
                    }
                    ui.add_enabled_ui(
                        self.pending.is_none()
                            && self.parameter_draft.is_none()
                            && self.feature_draft.is_none()
                            && self.sketch_draft.is_none()
                            && self.import_draft.is_none()
                            && self.placement_draft.is_none()
                            && self.move_draft.is_none()
                            && self.project_draft.is_none(),
                        |ui| {
                            if ui.button("Undo").on_hover_text("Undo (Ctrl+Z)").clicked() {
                                self.run_command(&ctx, keys::Command::Undo);
                            }
                            if ui.button("Redo").on_hover_text("Redo (Ctrl+Y)").clicked() {
                                self.run_command(&ctx, keys::Command::Redo);
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
                    } else if let Some((message, at)) = &self.delete_notice
                        && at.elapsed() < Duration::from_secs(6)
                    {
                        ui.label(message);
                    } else if self.saving {
                        ui.spinner();
                        ui.label("Building model…");
                    } else {
                        ui.label(RichText::new("Saved").color(ACCENT));
                    }
                    if let Some(message) = &self.export_message {
                        ui.label(message);
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
                    let chosen = self.selection().len();
                    if chosen > 1 {
                        ui.label(
                            RichText::new(format!("   ·   {chosen} parts selected"))
                                .small()
                                .color(MUTED),
                        );
                    }
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
            .default_size(220.)
            .size_range(160. ..=500.)
            .resizable(true)
            .frame(panel())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("project_panel_scroll")
                    .show(ui, |ui| self.tree(ui));
            });
        egui::Panel::right("inspect")
            .default_size(264.)
            .size_range(200. ..=640.)
            .resizable(true)
            .frame(panel())
            .show(ui, |ui| {
                // Keep the viewport boundary independent of the width of inspector values.
                let rect = ui.available_rect_before_wrap();
                let mut content = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                content.shrink_clip_rect(rect);
                ui.allocate_space(rect.size());
                egui::ScrollArea::vertical()
                    .id_salt("inspect_panel_scroll")
                    .show(&mut content, |ui| self.inspector(ui));
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
        self.move_panel(&ctx);
        self.press_panel(&ctx);
        self.command_search(&ctx);
        self.project_panel(&ctx, frame);
        self.discard_question(&ctx);
        self.poll_pick_info(&ctx);
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
        for _ in 0..5 {
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
        for _ in 0..5 {
            harness.step();
        }
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
        for _ in 0..5 {
            harness.step();
        }
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
            ("Plane through points", 6000.),
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
            harness.get_by_label("Model viewport").click();
            harness.step();
            let hole = matches!(tool, "Hole" | "Counterbore" | "Countersink");
            if tool.starts_with("Reference ")
                || matches!(tool, "Plane on face" | "Plane through points")
            {
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
                .or_else(|| (tool == "Plane through points").then_some("plane_through_points"))
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
        harness.get_by_label("Model viewport").click();
        harness.step();
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
            .build_eframe(|cc| {
                let mut shell = Shell::welcome(cc);
                shell.load_recent(directory.path().join("recent.json"));
                shell
            });
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
        drop(harness);
        let mut reopened = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1200., 800.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(|cc| {
                let mut shell = Shell::welcome(cc);
                shell.load_recent(directory.path().join("recent.json"));
                shell
            });
        for _ in 0..3 {
            reopened.step();
        }
        reopened.get_by_label("Recent projects");
        reopened.get_by_label("Untitled").click();
        for _ in 0..100 {
            reopened.step();
            if reopened.state().project_loaded {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::canonicalize(&reopened.state().root).unwrap(),
            std::fs::canonicalize(path).unwrap()
        );
    }
    use egui_kittest::kittest::Queryable;
    use faber_model::{Geometry, Part, Placement, Profile};

    #[test]
    fn visible_anchor_takes_the_point_nearest_the_centre() {
        let rect = egui::Rect::from_min_size(egui::pos2(0., 0.), egui::vec2(400., 400.));
        let inner = rect.shrink(40.);
        // The identity matrix maps a point to the normalised view: x and y in -1..1.
        let off = glam::Vec3::new(0.95, 0., 0.5);
        let far = glam::Vec3::new(0.5, 0., 0.5);
        let near = glam::Vec3::new(0.1, 0.1, 0.5);
        let behind = glam::Vec3::new(0., 0., 1.5);
        let chosen = visible_anchor(
            [off, far, near].into_iter(),
            glam::Mat4::IDENTITY,
            rect,
            inner,
        );
        assert_eq!(chosen, Some(near));
        let none = visible_anchor([off, behind].into_iter(), glam::Mat4::IDENTITY, rect, inner);
        assert_eq!(none, None);
    }

    #[test]
    fn section_handle_sits_on_the_visible_cut() {
        let dir = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(dir.path(), true, Some("Handle cut test".into())).unwrap();
        let geometry: Geometry<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
            "op": "history",
            "features": [{"id": "block", "geometry": {"op": "box", "size": [400, 400, 400]}}],
            "output": "block"
        }))
        .unwrap();
        let part = Part {
            material: None,
            name: "Block".into(),
            geometry,
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("block".into(), part).unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
        for _ in 0..3 {
            harness.step();
        }
        assert!(harness.state().section_on);
        // Cut through the box interior at x = 200. The box spans 0..400 on each axis.
        harness.state_mut().plane.offset = 200.;
        // Aim near a cap triangle centroid, away from the model centre (200, 200, 200).
        {
            let camera = &mut harness.state_mut().camera;
            camera.target = glam::Vec3::new(200., 267., 133.);
            camera.distance = 60.;
        }
        for _ in 0..3 {
            harness.step();
        }
        let view = harness.get_by_label("Model viewport").rect();
        let matrix = harness.state().camera.matrix(view.aspect_ratio());
        let centre = matrix.project_point3(glam::Vec3::new(200., 200., 200.));
        assert!(
            centre.x.abs() > 1. || centre.y.abs() > 1. || !(0.0..=1.0).contains(&centre.z),
            "The model centre must be off the view: {centre:?}"
        );
        let state = harness.state();
        let anchor = state.section_anchor.expect("The handle has no anchor");
        let normal = glam::Vec3::from(state.plane.normal.map(|x| x as f32)).normalize();
        let on_plane = normal.dot(anchor) - state.plane.offset as f32;
        assert!(
            on_plane.abs() < 1e-3,
            "Anchor {anchor:?} is off the cut plane"
        );
        assert!(
            (-1e-3..=400. + 1e-3).contains(&anchor.y) && (-1e-3..=400. + 1e-3).contains(&anchor.z),
            "Anchor {anchor:?} is off the cut face"
        );
        // The cut centre is in view. The handle must sit near it, not at the view edge.
        let seen = matrix.project_point3(anchor);
        let at =
            view.center() + egui::Vec2::new(seen.x * view.width(), -seen.y * view.height()) * 0.5;
        assert!(
            at.distance(view.center()) < view.height() * 0.25,
            "Handle centre {at:?} is not near the view centre"
        );
        let handle = harness
            .ctx
            .read_response(egui::Id::unique("section_offset_handle"))
            .unwrap()
            .rect;
        assert!(
            view.contains(handle.min) && view.contains(handle.max),
            "Handle {handle:?} is outside the view {view:?}"
        );
    }

    #[test]
    fn section_handle_stays_in_view_when_zoomed_in() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Handle test".into())).unwrap();
        let geometry: Geometry<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
            "op": "history",
            "features": [{"id": "block", "geometry": {"op": "box", "size": [400, 400, 400]}}],
            "output": "block"
        }))
        .unwrap();
        let part = Part {
            material: None,
            name: "Block".into(),
            geometry,
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("block".into(), part).unwrap();
        let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
        for _ in 0..3 {
            harness.step();
        }
        assert!(harness.state().section_on);
        // Zoom in on a point off the model. The model centre leaves the view.
        {
            let camera = &mut harness.state_mut().camera;
            camera.target = glam::Vec3::new(900., 0., 0.);
            camera.distance = 60.;
        }
        for _ in 0..3 {
            harness.step();
        }
        let view = harness.get_by_label("Model viewport").rect();
        let seen = harness
            .state()
            .camera
            .matrix(view.aspect_ratio())
            .project_point3(glam::Vec3::new(0., 0., 200.));
        assert!(
            seen.x.abs() > 1. || seen.y.abs() > 1. || !(0.0..=1.0).contains(&seen.z),
            "The model centre must be off the view: {seen:?}"
        );
        let handle = harness
            .ctx
            .read_response(egui::Id::unique("section_offset_handle"))
            .unwrap()
            .rect;
        assert!(
            view.contains(handle.min) && view.contains(handle.max),
            "Handle {handle:?} is outside the view {view:?}"
        );
    }

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
            material: None,
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
                suppressed: false,
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
            .with_size(egui::vec2(1440., 1100.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.get_by_label("Model viewport").click_secondary();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Move/Copy M").click();
        harness.step();
        for _ in 0..3 {
            harness.step();
        }
        assert_eq!(harness.state().move_draft.as_ref().unwrap().part, "bin");
        harness.get_by_label("Cancel").click();
        harness.step();
        harness.state_mut().selected = None;
        harness.state_mut().mesh_dirty = true;
        harness.step();
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
        harness.state_mut().cancel_window();
        harness.step();

        harness.get_by_label("PARAMETERS");
        harness.get_by_label("wall = 4");
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
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
        harness.get_by_label("Model viewport").click();
        for _ in 0..3 {
            harness.step();
        }
        assert_eq!(harness.state().selected.as_deref(), Some("bin"));
        harness.get_by_label("FEATURES");
        harness.get_by_label("walls");
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
        for _ in 0..3 {
            harness.step();
        }
        assert!(harness.state().section_on);
        let camera = harness.state().camera;
        let handle_id = egui::Id::unique("section_offset_handle");
        harness.step();
        let initial = harness.ctx.read_response(handle_id).unwrap().rect.center();
        let normal = glam::Vec3::from(harness.state().plane.normal.map(|x| x as f32));
        harness.state_mut().camera.target += normal.any_orthonormal_vector() * 35.;
        harness.state_mut().camera.distance *= 0.8;
        for _ in 0..3 {
            harness.step();
        }
        let moved = harness.ctx.read_response(handle_id).unwrap().rect.center();
        assert!(initial.distance(moved) > 10.);
        harness.state_mut().camera = camera;
        for _ in 0..3 {
            harness.step();
        }
        let restored = harness.ctx.read_response(handle_id).unwrap().rect.center();
        assert!(initial.distance(restored) < 0.01);
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
        for (handle, rotation) in [
            ("section_offset_handle", false),
            ("section_tilt_handle", true),
            ("section_turn_handle", true),
        ] {
            for _ in 0..5 {
                harness.step();
            }
            let centre = harness
                .ctx
                .read_response(egui::Id::unique(handle))
                .unwrap()
                .rect
                .center();
            let before = harness.state().plane;
            let yaw = harness.state().camera.yaw;
            let centre = if rotation {
                centre
            } else {
                // Press the shaft outside the arrow tip hit box.
                let rect = harness
                    .get_by_label("Model viewport")
                    .rect()
                    .expand2(Vec2::new(5., 0.));
                let matrix = harness.state().camera.matrix(rect.aspect_ratio());
                let n = glam::Vec3::from(before.normal.map(|x| x as f32));
                let projected = matrix.project_point3(n) - matrix.project_point3(glam::Vec3::ZERO);
                centre
                    - Vec2::new(projected.x * rect.width(), -projected.y * rect.height())
                        .normalized()
                        * 22.
            };
            harness.hover_at(centre);
            harness.step();
            harness.drag_at(centre);
            harness.step();
            let end = centre + Vec2::new(16., -21.);
            harness.hover_at(end);
            harness.step();
            harness.hover_at(centre);
            harness.step();
            assert!((harness.state().plane.offset - before.offset).abs() < 0.0001);
            assert!(
                glam::DVec3::from(harness.state().plane.normal).distance(before.normal.into())
                    < 0.00001
            );
            harness.hover_at(end);
            harness.step();
            assert!(harness.state().caps.is_some());
            assert_eq!(harness.state().cap_plane, Some(harness.state().plane));
            assert!(harness.state().caps.is_some());
            let pivot = harness.state().section_pivot.unwrap();
            harness.drop_at(end);
            for _ in 0..3 {
                harness.step();
            }
            assert_eq!(harness.state().section_pivot, Some(pivot));
            if rotation {
                let normal = glam::DVec3::from(harness.state().plane.normal);
                assert!(
                    (normal.dot(pivot.as_dvec3()) - harness.state().plane.offset).abs() < 0.0001
                );
                assert!(normal.angle_between(before.normal.into()) < 0.14);
            }
            assert_eq!(harness.state().camera.yaw, yaw);
            if rotation {
                assert_ne!(harness.state().plane.normal, before.normal);
            } else {
                assert_ne!(harness.state().plane.offset, before.offset);
            }
        }
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
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
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
        harness.state_mut().cancel_window();
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
        let saved = faber_wiring::project_fingerprint(dir.path()).unwrap();
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
        assert_eq!(
            faber_wiring::project_fingerprint(dir.path()).unwrap(),
            saved
        );
        assert!((harness.state().bodies[0].bounds[1][2] - 160.).abs() < 1e-5);
        harness.get_by_label("Cancel feature").click();
        harness.step();
        harness.get_by_label("New sketch").click();
        for _ in 0..3 {
            harness.step();
        }
        for _ in 0..5 {
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
        harness.get_by_label("Import model").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Source file").focus();
        harness.step();
        harness
            .get_by_label("Source file")
            .type_text(&dir.path().join("out/bin.step").to_string_lossy());
        harness.step();
        harness.get_by_label("Import file").click();
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
        harness.get_by_label("Exact placement").click();
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
        let original_file = faber_wiring::project_fingerprint(dir.path()).unwrap();
        let next_directory = tempfile::tempdir().unwrap();
        let new_path = next_directory.path().join("new_project");
        for (button, path, count) in [
            ("New project", new_path.as_path(), 0),
            ("Open project", dir.path(), 3),
        ] {
            if button == "Open project" {
                harness.state_mut().picked_file = Some(path.join("faber.json"));
            }
            harness.state_mut().last_poll = Instant::now();
            harness.step();
            harness.get_by_label(button).click();
            for _ in 0..3 {
                harness.step();
            }
            if button == "New project" {
                harness.get_by_label("Project folder").focus();
                harness.step();
                harness
                    .get_by_label("Project folder")
                    .type_text(&path.to_string_lossy());
                harness.step();
                harness.get_by_label("Load project").click();
                harness.step();
            } else {
                assert!(harness.query_by_label("Project folder").is_none());
            }
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
            faber_wiring::project_fingerprint(dir.path()).unwrap(),
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
            let file = faber_wiring::project_fingerprint(&state.root).ok();
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
        serde_json::to_value(faber_wiring::read(root).unwrap().0).unwrap()
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
            material: None,
            name: "Bin".into(),
            geometry: Geometry::History {
                features: vec![faber_model::Feature {
                    id: "walls".into(),
                    geometry,
                    suppressed: false,
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
        let analysis = serde_json::json!({
            "schema": 1,
            "sections": {"side": {"plane": {"kind": "plane", "origin": [0, 0, 0], "normal": [0, 1, 0]}}}
        });
        std::fs::write(
            dir.path().join("analysis.json"),
            serde_json::to_string_pretty(&analysis).unwrap(),
        )
        .unwrap();
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
            material: None,
            name: "Bin".into(),
            geometry: Geometry::History {
                features: vec![
                    faber_model::Feature {
                        id: "walls".into(),
                        geometry: pocket,
                        suppressed: false,
                    },
                    faber_model::Feature {
                        id: "ref".into(),
                        geometry: Geometry::Plane {
                            placement: Placement {
                                translation: [0., 0., 10.],
                                rotation: [0., 0., 0.],
                            },
                        },
                        suppressed: false,
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
        let analysis = serde_json::json!({
            "schema": 1,
            "sections": {"lid_plane": {"plane": {"kind": "reference", "part": "bin", "feature": "ref", "offset": 5}}}
        });
        std::fs::write(
            dir.path().join("analysis.json"),
            serde_json::to_string_pretty(&analysis).unwrap(),
        )
        .unwrap();
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

#[cfg(test)]
mod windows_tests;

#[cfg(test)]
mod marking_menu_tests;
