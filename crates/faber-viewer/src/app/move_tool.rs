//! Move and copy tool for parts.
//!
//! The preview moves the body with a display pose only.
//! OK sets the placement of the part in one edit.

use super::*;
use faber_core::{AlignMode, Axis, Motion, align_shift, refuse_purchased_copy, tidy};
use faber_model::{Edit, Part, Placement, Scalar};
use glam::{DMat3, DMat4, DQuat, DVec3, EulerRot, Mat4};
use std::borrow::Cow;
use std::path::Path;

const AXES: [DVec3; 3] = [DVec3::X, DVec3::Y, DVec3::Z];
const AXIS_COLOURS: [Color32; 3] = [
    Color32::from_rgb(220, 70, 70),
    Color32::from_rgb(80, 190, 90),
    Color32::from_rgb(70, 130, 230),
];
const SNAP_MM: f64 = 1.;
const SNAP_DEGREES: f64 = 15.;
const RING_PX_PER_DEGREE: f32 = 2.;
const RING_SCALE: f64 = 0.8;
const GRIP_RANGE: f32 = 9.;
const SEGMENTS: usize = 48;
/// Name of the world origin in the align target list.
const ORIGIN: &str = "origin";
/// Screen radius for a vertex snap, in pixels.
const SNAP_PX: f32 = 12.;
/// A surface point this close behind a vertex still counts as visible, in mm.
const SURFACE_TOLERANCE_MM: f64 = 0.5;
/// Width of the command panel, in pixels.
const PANEL_WIDTH: f32 = 280.;
/// Space between the command panel and the viewport edge, in pixels.
const PANEL_MARGIN: f32 = 12.;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum MoveMode {
    Free,
    Translate,
    Rotate,
    PointToPoint,
    Position,
    Align,
}

/// The point that the triad and the rings turn about.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PivotMode {
    Centre,
    Origin,
    World,
}

/// The point that the next viewport click sets.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PickStage {
    From,
    To,
}

/// Text of the two points of point to point. Values are in mm.
struct Points {
    from: [String; 3],
    to: [String; 3],
    picking: Option<PickStage>,
    note: Option<String>,
}

impl Points {
    fn new() -> Self {
        Self {
            from: zero_text(),
            to: zero_text(),
            picking: None,
            note: None,
        }
    }
}

fn zero_text() -> [String; 3] {
    ["0".into(), "0".into(), "0".into()]
}

/// Settings of the align mode.
struct AlignPanel {
    target: String,
    axes: [bool; 3],
    mode: AlignMode,
    gap: String,
}

impl AlignPanel {
    /// Targets the first other part, or the origin when there is none.
    fn new(part: &str, parts: &BTreeMap<String, Part<Scalar>>) -> Self {
        let target = parts
            .keys()
            .find(|id| id.as_str() != part)
            .cloned()
            .unwrap_or_else(|| ORIGIN.to_owned());
        Self {
            target,
            axes: [false, false, true],
            mode: AlignMode::MinToMax,
            gap: "0".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Grip {
    Arm(usize),
    Ring(usize),
}

#[derive(Clone, Copy)]
struct Drag {
    grip: Grip,
    // Unit screen direction. For a ring, the direction of a growing angle.
    dir: Vec2,
    // Pixels per mm for an arm. Pixels per degree for a ring.
    px_per: f32,
    // Translate and turn before the drag.
    start: (DVec3, DQuat),
}

pub(super) struct MoveDraft {
    expected: ProjectData,
    pub(super) part: String,
    base: Placement<f64>,
    // World centre of the bounding box before the move.
    centre: DVec3,
    // Arm length in mm.
    size: f64,
    mode: MoveMode,
    // Shift in mm.
    translate: DVec3,
    // Turn of the free mode. The angle fields show its Euler angles.
    turn: DQuat,
    // Distance fields, in mm.
    fields: [String; 3],
    applied_fields: [String; 3],
    // Angle fields, in degrees.
    angles: [String; 3],
    applied_angles: [String; 3],
    axis: usize,
    angle: String,
    applied_angle: (usize, String),
    pivot_mode: PivotMode,
    position: [String; 3],
    copy: bool,
    error: Option<&'static str>,
    drag: Option<Drag>,
    // World boxes of all bodies before the move.
    bounds: BTreeMap<String, [[f64; 3]; 2]>,
    points: Points,
    align: AlignPanel,
    // Viewport rect, set by the canvas each frame.
    viewport: Option<egui::Rect>,
    // Rect of the command panel, set each frame.
    panel_rect: Option<egui::Rect>,
}

/// Splits a placement rotation into a quaternion. R = Rz * Ry * Rx.
pub(super) fn placement_rotation(rotation: [f64; 3]) -> DQuat {
    let [x, y, z] = rotation.map(f64::to_radians);
    DQuat::from_euler(EulerRot::ZYX, z, y, x)
}

/// Splits a rotation into X, Y and Z degrees. R = Rz * Ry * Rx.
pub(super) fn euler_degrees(rotation: DQuat) -> [f64; 3] {
    let m = DMat3::from_quat(rotation);
    let r = |row: usize, col: usize| m.col(col)[row];
    let cos_y = r(0, 0).hypot(r(1, 0));
    let (x, z) = if cos_y > 1e-9 {
        (r(2, 1).atan2(r(2, 2)), r(1, 0).atan2(r(0, 0)))
    } else {
        // Gimbal lock. Z is zero and X takes the turn.
        (-r(1, 2).atan2(r(1, 1)), 0.)
    };
    [x, (-r(2, 0)).atan2(cos_y), z].map(f64::to_degrees)
}

/// Moves a placement. The turn and the shift act about the pivot.
/// Each saved value is tidy, so a right angle saves as exactly 90.
pub(super) fn compose(
    base: Placement<f64>,
    pivot: DVec3,
    translate: DVec3,
    turn: DQuat,
) -> Placement<f64> {
    let origin = DVec3::from(base.translation);
    let moved = turn * (origin - pivot) + pivot + translate;
    Placement {
        translation: moved.to_array().map(tidy),
        rotation: euler_degrees(turn * placement_rotation(base.rotation)).map(tidy),
    }
}

#[cfg(test)]
fn placement_matrix(placement: &Placement<f64>) -> DMat4 {
    DMat4::from_rotation_translation(
        placement_rotation(placement.rotation),
        DVec3::from(placement.translation),
    )
}

fn fixed(values: &[Scalar; 3]) -> Option<[f64; 3]> {
    let mut out = [0.; 3];
    for (slot, value) in out.iter_mut().zip(values) {
        match value {
            Scalar::Number(number) => *slot = *number,
            Scalar::Expression(_) => return None,
        }
    }
    Some(out)
}

/// Text of a mm or degree value. Two decimals at most. Negative zero prints as zero.
fn mm_text(value: f64) -> String {
    let rounded = (value * 100.).round() / 100.;
    if rounded == 0. {
        "0".into()
    } else {
        format!("{rounded}")
    }
}

/// Rounds a value to the nearest multiple of a step.
fn snapped(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

fn parse_vector(text: &[String; 3]) -> Option<DVec3> {
    let mut out = [0.; 3];
    for (slot, value) in out.iter_mut().zip(text) {
        let number: f64 = value.trim().parse().ok()?;
        if !number.is_finite() {
            return None;
        }
        *slot = number;
    }
    Some(DVec3::from(out))
}

/// Screen position of a world point. None when the point is behind the camera.
fn screen_point(rect: egui::Rect, matrix: Mat4, point: DVec3) -> Option<egui::Pos2> {
    let clip = matrix * point.as_vec3().extend(1.);
    (clip.w > 0.).then(|| {
        rect.center()
            + Vec2::new(
                clip.x / clip.w * rect.width(),
                -clip.y / clip.w * rect.height(),
            ) * 0.5
    })
}

/// Shift that aligns a moving box with a target box on the chosen axes.
/// Each axis uses the core `align_shift`.
fn align_translation(
    moving: [[f64; 3]; 2],
    target: [[f64; 3]; 2],
    axes: [bool; 3],
    mode: AlignMode,
    gap: f64,
) -> DVec3 {
    let mut shift = DVec3::ZERO;
    for (index, axis) in [Axis::X, Axis::Y, Axis::Z].into_iter().enumerate() {
        if axes[index] {
            shift[index] = align_shift(moving, target, axis, mode, gap);
        }
    }
    shift
}

fn copy_id(parts: &BTreeMap<String, Part<Scalar>>, id: &str) -> String {
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            format!("{id}_copy")
        } else {
            format!("{id}_copy{n}")
        };
        if !parts
            .keys()
            .any(|existing| existing.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        n += 1;
    }
}

/// Saves the new placement, or a copy of the part, in one edit.
fn commit(
    root: &Path,
    expected: &ProjectData,
    id: &str,
    placement: Placement<Scalar>,
    copy: bool,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Open the move tool again.".into());
    }
    let mut part = expected.parts.get(id).ok_or("Part is missing")?.clone();
    if copy {
        refuse_purchased_copy(id, &part)?;
    }
    part.placement = placement;
    let target = if copy {
        part.name = format!("{} copy", part.name);
        copy_id(&expected.parts, id)
    } else {
        id.to_owned()
    };
    let operations = vec![Edit::PutPart { id: target, part }];
    if strict {
        project.batch_strict(operations)?;
    } else {
        project.batch(operations)?;
    }
    Ok((project.data, project.bodies))
}

impl MoveDraft {
    fn sync_fields(&mut self) {
        self.fields = self.translate.to_array().map(mm_text);
        self.applied_fields = self.fields.clone();
        self.angles = euler_degrees(self.turn).map(mm_text);
        self.applied_angles = self.angles.clone();
        self.error = None;
    }

    /// Reads typed values. Each value applies once, when its text changes.
    fn read_fields(&mut self) {
        match self.mode {
            MoveMode::Free => {
                let errors = [self.read_translate(), self.read_turn()];
                self.error = errors.into_iter().flatten().next();
            }
            MoveMode::Translate => self.error = self.read_translate(),
            MoveMode::Rotate if (self.axis, self.angle.clone()) != self.applied_angle => {
                match self.angle.trim().parse::<f64>() {
                    Ok(degrees) if degrees.is_finite() => {
                        self.turn = DQuat::from_axis_angle(AXES[self.axis], degrees.to_radians());
                        self.applied_angle = (self.axis, self.angle.clone());
                        self.error = None;
                    }
                    _ => self.error = Some("Enter a number of degrees."),
                }
            }
            MoveMode::PointToPoint => self.read_points(),
            MoveMode::Position => self.read_position(),
            MoveMode::Align => self.read_align(),
            MoveMode::Rotate => {}
        }
    }

    /// Reads the distance fields when their text changed. Returns an error text when a value is bad.
    fn read_translate(&mut self) -> Option<&'static str> {
        if self.fields == self.applied_fields {
            return None;
        }
        match parse_vector(&self.fields) {
            Some(value) => {
                self.translate = value;
                self.applied_fields = self.fields.clone();
                None
            }
            None => Some("Enter numbers in mm."),
        }
    }

    /// Reads the angle fields when their text changed. Returns an error text when a value is bad.
    fn read_turn(&mut self) -> Option<&'static str> {
        if self.angles == self.applied_angles {
            return None;
        }
        match parse_vector(&self.angles) {
            Some(value) => {
                self.turn = placement_rotation(value.to_array());
                self.applied_angles = self.angles.clone();
                None
            }
            None => Some("Enter numbers in degrees."),
        }
    }

    /// The bounding box centre moves to the typed point. The core `Motion::Position` gives the shift.
    fn read_position(&mut self) {
        let Some(point) = parse_vector(&self.position) else {
            self.translate = DVec3::ZERO;
            self.error = Some("Enter numbers in mm.");
            return;
        };
        let Some(moving) = self.bounds.get(&self.part).copied() else {
            self.translate = DVec3::ZERO;
            self.error = Some("The part has no solid.");
            return;
        };
        match Motion::Position(point.to_array()).apply(&self.base, moving) {
            Ok(moved) => {
                self.translate =
                    DVec3::from(moved.translation) - DVec3::from(self.base.translation);
                self.error = None;
            }
            Err(_) => {
                self.translate = DVec3::ZERO;
                self.error = Some("Enter numbers in mm.");
            }
        }
    }

    /// World point that the triad and the rings turn about.
    fn pivot_point(&self) -> DVec3 {
        match self.pivot_mode {
            PivotMode::Centre => self.centre,
            PivotMode::Origin => DVec3::from(self.base.translation),
            PivotMode::World => DVec3::ZERO,
        }
    }

    /// Shift and turn of the current mode. The other values are not shown and not saved.
    fn motion(&self) -> (DVec3, DQuat) {
        match self.mode {
            MoveMode::Free => (self.translate, self.turn),
            MoveMode::Rotate => (DVec3::ZERO, self.turn),
            _ => (self.translate, DQuat::IDENTITY),
        }
    }

    /// Arrows move the part in Free and Move modes. Rings turn it in Free mode.
    fn allows(&self, grip: Grip) -> bool {
        match grip {
            Grip::Arm(_) => matches!(self.mode, MoveMode::Free | MoveMode::Translate),
            Grip::Ring(_) => self.mode == MoveMode::Free,
        }
    }

    /// Changes the mode. Each mode keeps only the values that it shows.
    fn select_mode(&mut self, mode: MoveMode) {
        self.mode = mode;
        self.points.picking = None;
        if mode != MoveMode::Free {
            self.turn = DQuat::IDENTITY;
        }
        if mode == MoveMode::Rotate {
            self.translate = DVec3::ZERO;
            // Forces a new read of the angle.
            self.applied_angle = (usize::MAX, String::new());
        }
        self.sync_fields();
    }

    /// The shift is the to point minus the from point.
    fn read_points(&mut self) {
        match (
            parse_vector(&self.points.from),
            parse_vector(&self.points.to),
        ) {
            (Some(from), Some(to)) => {
                self.translate = to - from;
                self.error = None;
            }
            _ => {
                self.translate = DVec3::ZERO;
                self.error = Some("Pick or type both points.");
            }
        }
    }

    fn read_align(&mut self) {
        match self.align_offset() {
            Ok(shift) => {
                self.translate = shift;
                self.error = None;
            }
            Err(error) => {
                self.translate = DVec3::ZERO;
                self.error = Some(error);
            }
        }
    }

    fn align_offset(&self) -> Result<DVec3, &'static str> {
        let moving = self
            .bounds
            .get(&self.part)
            .copied()
            .ok_or("The part has no solid.")?;
        if self.align.target == self.part {
            return Err("A part cannot align to itself.");
        }
        let target = if self.align.target == ORIGIN {
            [[0.; 3]; 2]
        } else {
            self.bounds
                .get(&self.align.target)
                .copied()
                .ok_or("Choose a target part.")?
        };
        if !self.align.axes.contains(&true) {
            return Err("Choose at least one axis.");
        }
        let gap = if self.align.mode == AlignMode::MinToMax {
            self.align
                .gap
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|gap| gap.is_finite())
                .ok_or("Enter a number in mm.")?
        } else {
            0.
        };
        Ok(align_translation(
            moving,
            target,
            self.align.axes,
            self.align.mode,
            gap,
        ))
    }

    /// Points for the marks in the viewport. The from mark moves with the part.
    fn marks(&self) -> [Option<DVec3>; 2] {
        if self.mode != MoveMode::PointToPoint {
            return [None, None];
        }
        [
            parse_vector(&self.points.from).map(|from| from + self.translate),
            parse_vector(&self.points.to),
        ]
    }

    /// Fields of the point to point mode.
    fn points_ui(&mut self, ui: &mut egui::Ui) {
        let picking = self.points.picking;
        let mut next = None;
        egui::Grid::new("point_fields").show(ui, |ui| {
            for (stage, caption, hint, values) in [
                (
                    PickStage::From,
                    "From (mm)",
                    "Pick or type the from point. Click a vertex or a face of the selected part.",
                    &mut self.points.from,
                ),
                (
                    PickStage::To,
                    "To (mm)",
                    "Pick or type the to point. Click a vertex or a face of another part, or the grid.",
                    &mut self.points.to,
                ),
            ] {
                ui.label(caption);
                ui.horizontal(|ui| {
                    for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values.iter_mut()) {
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .desired_width(64.)
                                .hint_text(axis),
                        );
                    }
                });
                let active = picking == Some(stage);
                if ui.selectable_label(active, "Pick").on_hover_text(hint).clicked() {
                    next = Some(if active { None } else { Some(stage) });
                }
                ui.end_row();
            }
        });
        if let Some(next) = next {
            self.points.picking = next;
        }
        if let Some(note) = &self.points.note {
            ui.label(note.as_str());
        }
    }

    /// Fields of the align mode. Targets come from the part list.
    fn align_ui(&mut self, ui: &mut egui::Ui, targets: &[String]) {
        ui.horizontal(|ui| {
            ui.label("Target");
            egui::ComboBox::from_id_salt("align_target")
                .selected_text(self.align.target.as_str())
                .show_ui(ui, |ui| {
                    for name in targets {
                        ui.selectable_value(&mut self.align.target, name.clone(), name.as_str());
                    }
                });
        });
        ui.horizontal(|ui| {
            for (index, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                ui.checkbox(&mut self.align.axes[index], name);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (mode, label, hint) in [
                (AlignMode::Min, "Min", "Min faces meet."),
                (AlignMode::Max, "Max", "Max faces meet."),
                (AlignMode::Centre, "Centre", "Centres meet."),
                (
                    AlignMode::MinToMax,
                    "Stack on top",
                    "Stack on top: the min face of the part meets the max face of the target.",
                ),
            ] {
                ui.selectable_value(&mut self.align.mode, mode, label)
                    .on_hover_text(hint);
            }
        });
        let stacking = self.align.mode == AlignMode::MinToMax;
        ui.add_enabled_ui(stacking, |ui| {
            ui.horizontal(|ui| {
                let caption = ui.label("Gap (mm)");
                ui.text_edit_singleline(&mut self.align.gap)
                    .labelled_by(caption.id);
            });
        });
    }

    fn result(&self) -> Placement<f64> {
        let (translate, turn) = self.motion();
        compose(self.base.clone(), self.pivot_point(), translate, turn)
    }
}

/// Small muted caption above a row.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(MUTED));
}

/// Labelled text fields. Each field takes its caption as its label.
fn field_grid(ui: &mut egui::Ui, id: &str, captions: [&str; 3], values: &mut [String; 3]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([8., 4.])
        .show(ui, |ui| {
            for (text, value) in captions.into_iter().zip(values.iter_mut()) {
                let label = ui.label(text);
                ui.add(egui::TextEdit::singleline(value).desired_width(150.))
                    .labelled_by(label.id);
                ui.end_row();
            }
        });
}

/// Screen geometry of the move triad.
struct Triad {
    centre: egui::Pos2,
    tips: [egui::Pos2; 3],
    dirs: [Vec2; 3],
    px_per: [f32; 3],
    rings: [Vec<egui::Pos2>; 3],
}

fn ring_basis(axis: usize) -> (DVec3, DVec3) {
    (AXES[(axis + 1) % 3], AXES[(axis + 2) % 3])
}

fn triad_at(rect: egui::Rect, matrix: Mat4, pivot: DVec3, size: f64) -> Option<Triad> {
    let project = |point: DVec3| {
        let clip = matrix * point.as_vec3().extend(1.);
        (clip.w > 0.).then(|| {
            rect.center()
                + Vec2::new(
                    clip.x / clip.w * rect.width(),
                    -clip.y / clip.w * rect.height(),
                ) * 0.5
        })
    };
    let centre = project(pivot)?;
    let mut tips = [centre; 3];
    let mut dirs = [Vec2::X; 3];
    let mut px_per = [1.; 3];
    for axis in 0..3 {
        let tip = project(pivot + AXES[axis] * size)?;
        let delta = tip - centre;
        let distance = delta.length();
        if distance > 0.5 {
            dirs[axis] = delta / distance;
        }
        px_per[axis] = (distance / size as f32).max(0.05);
        tips[axis] = tip;
    }
    let rings = std::array::from_fn(|axis| {
        let (u, v) = ring_basis(axis);
        (0..=SEGMENTS)
            .filter_map(|step| {
                let angle = step as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
                project(pivot + (u * angle.cos() + v * angle.sin()) * size * RING_SCALE)
            })
            .collect()
    });
    Some(Triad {
        centre,
        tips,
        dirs,
        px_per,
        rings,
    })
}

fn segment_distance(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let line = b - a;
    let t = ((p - a).dot(line) / line.length_sq().max(1e-4)).clamp(0., 1.);
    p.distance(a + line * t)
}

/// Finds the grip under a screen point. Returns the grip, its direction and its scale.
fn grip_at(triad: &Triad, pos: egui::Pos2) -> Option<(Grip, Vec2, f32)> {
    let mut best: Option<(f32, Grip, Vec2, f32)> = None;
    let mut consider = |distance: f32, grip: Grip, dir: Vec2, px_per: f32| {
        if distance <= GRIP_RANGE && best.is_none_or(|(d, ..)| distance < d) {
            best = Some((distance, grip, dir, px_per));
        }
    };
    for axis in 0..3 {
        consider(
            segment_distance(pos, triad.centre, triad.tips[axis]),
            Grip::Arm(axis),
            triad.dirs[axis],
            triad.px_per[axis],
        );
    }
    for axis in 0..3 {
        for pair in triad.rings[axis].windows(2) {
            let line = pair[1] - pair[0];
            let Some(dir) = (line.length() > 0.01).then(|| line.normalized()) else {
                continue;
            };
            consider(
                segment_distance(pos, pair[0], pair[1]),
                Grip::Ring(axis),
                dir,
                RING_PX_PER_DEGREE,
            );
        }
    }
    best.map(|(_, grip, dir, px_per)| (grip, dir, px_per))
}

impl Shell {
    /// Opens the move tool for a part with plain numbers.
    pub(super) fn start_move(&mut self, id: &str) {
        self.start_move_in(id, MoveMode::Free);
    }

    /// Opens the move tool in one mode.
    pub(super) fn start_move_in(&mut self, id: &str, mode: MoveMode) {
        if !self.placement_available() {
            return;
        }
        let Some(part) = self.data.parts.get(id) else {
            return;
        };
        let (Some(translation), Some(rotation)) = (
            fixed(&part.placement.translation),
            fixed(&part.placement.rotation),
        ) else {
            self.error = Some(
                "This part uses parameter values. Move/Copy refuses it. Use Move / rotate instead."
                    .into(),
            );
            return;
        };
        let (pivot, size) = match self.bodies.iter().find(|body| body.id == id) {
            Some(body) => {
                let low = DVec3::from(body.bounds[0]);
                let high = DVec3::from(body.bounds[1]);
                (
                    (low + high) * 0.5,
                    ((high - low).max_element() * 0.5).max(5.),
                )
            }
            None => (DVec3::from(translation), 5.),
        };
        self.move_draft = Some(MoveDraft {
            expected: self.data.clone(),
            part: id.to_owned(),
            base: Placement {
                translation,
                rotation,
            },
            centre: pivot,
            size,
            mode,
            translate: DVec3::ZERO,
            turn: DQuat::IDENTITY,
            fields: zero_text(),
            applied_fields: zero_text(),
            angles: zero_text(),
            applied_angles: zero_text(),
            axis: 2,
            angle: "0".into(),
            applied_angle: (2, "0".into()),
            pivot_mode: PivotMode::Centre,
            position: pivot.to_array().map(mm_text),
            copy: false,
            error: None,
            drag: None,
            bounds: self
                .bodies
                .iter()
                .map(|body| (body.id.clone(), body.bounds))
                .collect(),
            points: Points::new(),
            align: AlignPanel::new(id, &self.data.parts),
            viewport: None,
            panel_rect: None,
        });
        self.error = None;
        self.dirty = true;
    }

    pub(super) fn cancel_move(&mut self) {
        self.move_draft = None;
        self.error = None;
        self.dirty = true;
    }

    /// Starts the save in a background thread.
    fn confirm_move(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        let Some(draft) = &mut self.move_draft else {
            return;
        };
        // Typed values count even when the panel has not read them yet.
        draft.read_fields();
        if draft.error.is_some() {
            return;
        }
        let result = draft.result();
        let placement = Placement {
            translation: result.translation.map(Scalar::Number),
            rotation: result.rotation.map(Scalar::Number),
        };
        let root = self.root.clone();
        let expected = draft.expected.clone();
        let part = draft.part.clone();
        let copy = draft.copy;
        let strict = self.strict_edits;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        std::thread::spawn(move || {
            let _ = tx.send(commit(&root, &expected, &part, placement, copy, strict));
            ctx.request_repaint();
        });
    }

    /// Enter confirms and Escape cancels.
    pub(super) fn move_keys(&mut self, ctx: &egui::Context) {
        if self.move_draft.is_none() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_move();
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.confirm_move(ctx);
        }
    }

    /// Command panel at the right of the viewport. It does not change the viewport layout.
    pub(super) fn move_panel(&mut self, ctx: &egui::Context) {
        let Some(part_id) = self.move_draft.as_ref().map(|draft| draft.part.clone()) else {
            return;
        };
        let part_name = self
            .data
            .parts
            .get(&part_id)
            .map_or_else(|| part_id.clone(), |part| part.name.clone());
        let targets: Vec<String> = std::iter::once(ORIGIN.to_owned())
            .chain(self.data.parts.keys().filter(|id| **id != part_id).cloned())
            .collect();
        let Some(draft) = &mut self.move_draft else {
            return;
        };
        draft.read_fields();
        let viewport = draft.viewport.unwrap_or_else(|| ctx.content_rect());
        let anchor = viewport.right_top() + Vec2::new(-PANEL_WIDTH - PANEL_MARGIN, PANEL_MARGIN);
        let mut apply = false;
        let mut cancel = false;
        let shown = egui::Window::new("MOVE/COPY")
            .fixed_pos(anchor)
            .min_width(PANEL_WIDTH)
            .max_width(PANEL_WIDTH)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .show(ctx, |ui| {
                caption(ui, "Object");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(part_name.as_str()).strong());
                    caption(ui, "Bodies");
                });
                ui.add_space(4.);
                caption(ui, "Move type");
                ui.horizontal_wrapped(|ui| {
                    for (mode, label, hint) in [
                        (
                            MoveMode::Free,
                            "Free",
                            "Free move. Drag an arrow to move the part. Drag a ring to turn it.",
                        ),
                        (
                            MoveMode::Translate,
                            "Move",
                            "Translate. Type the distance for each axis, in mm.",
                        ),
                        (
                            MoveMode::Rotate,
                            "Rotate",
                            "Rotate. Type the axis and the angle. The angle replaces the free turn.",
                        ),
                        (
                            MoveMode::PointToPoint,
                            "Point",
                            "Point to point. The part moves by the shift from the from point to the to point.",
                        ),
                        (
                            MoveMode::Position,
                            "Position",
                            "Point to position. The centre of the bounding box moves to the typed point.",
                        ),
                        (
                            MoveMode::Align,
                            "Align",
                            "Align. Line up the part with another part or the origin.",
                        ),
                    ] {
                        if ui
                            .selectable_label(draft.mode == mode, label)
                            .on_hover_text(hint)
                            .clicked()
                        {
                            draft.select_mode(mode);
                        }
                    }
                });
                ui.add_space(4.);
                match draft.mode {
                    MoveMode::Free => {
                        caption(ui, "Distance (mm)");
                        field_grid(
                            ui,
                            "free_distance",
                            ["X distance", "Y distance", "Z distance"],
                            &mut draft.fields,
                        );
                        caption(ui, "Angle (deg)");
                        field_grid(
                            ui,
                            "free_angle",
                            ["X angle", "Y angle", "Z angle"],
                            &mut draft.angles,
                        );
                    }
                    MoveMode::Translate => {
                        caption(ui, "Distance (mm)");
                        field_grid(
                            ui,
                            "move_distance",
                            ["X distance", "Y distance", "Z distance"],
                            &mut draft.fields,
                        );
                    }
                    MoveMode::Rotate => {
                        ui.horizontal(|ui| {
                            ui.label("Axis");
                            for (index, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                                ui.selectable_value(&mut draft.axis, index, name);
                            }
                        });
                        ui.horizontal(|ui| {
                            let caption = ui.label("Angle (deg)");
                            ui.text_edit_singleline(&mut draft.angle)
                                .labelled_by(caption.id);
                        });
                    }
                    MoveMode::PointToPoint => draft.points_ui(ui),
                    MoveMode::Position => {
                        caption(ui, "Centre of bounding box (mm)");
                        field_grid(
                            ui,
                            "move_position",
                            ["X position", "Y position", "Z position"],
                            &mut draft.position,
                        );
                    }
                    MoveMode::Align => draft.align_ui(ui, &targets),
                }
                ui.add_space(4.);
                caption(ui, "Pivot");
                let pivot_label = match draft.pivot_mode {
                    PivotMode::Centre => "Bounding box centre",
                    PivotMode::Origin => "Part origin",
                    PivotMode::World => "World origin",
                };
                egui::ComboBox::from_id_salt("pivot_mode")
                    .selected_text(pivot_label)
                    .width(PANEL_WIDTH - 24.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.pivot_mode,
                            PivotMode::Centre,
                            "Bounding box centre",
                        );
                        ui.selectable_value(&mut draft.pivot_mode, PivotMode::Origin, "Part origin");
                        ui.selectable_value(&mut draft.pivot_mode, PivotMode::World, "World origin");
                    });
                ui.add_space(4.);
                ui.checkbox(&mut draft.copy, "Create copy");
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(
                            self.pending.is_none() && draft.error.is_none(),
                            egui::Button::new("OK"),
                        )
                        .on_hover_text("Enter: OK. Saves the move in one edit.")
                        .clicked();
                    cancel = ui
                        .button("Cancel")
                        .on_hover_text("Escape: Cancel. The part does not move.")
                        .clicked();
                    ui.label("?").on_hover_text(
                        "Snap: 1 mm and 15 degrees. Hold Shift for no snap.",
                    );
                    if self.saving {
                        ui.spinner();
                    }
                });
                if let Some(error) = draft.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        if let Some(inner) = shown {
            draft.panel_rect = Some(inner.response.rect);
        }
        if apply {
            self.confirm_move(ctx);
        }
        if cancel {
            self.cancel_move();
        }
    }

    /// Handles the triad. Returns true while a triad drag runs.
    pub(super) fn move_canvas(&mut self, ui: &egui::Ui, rect: egui::Rect) -> bool {
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let section_busy = self.section_drag.is_some();
        let picked = self.pick_click(ui, rect);
        let Some(draft) = self.move_draft.as_mut() else {
            return false;
        };
        draft.viewport = Some(rect);
        let triad = triad_at(
            rect,
            matrix,
            draft.pivot_point() + draft.motion().0,
            draft.size,
        );
        let (down, pressed, shift) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_pressed(),
                i.modifiers.shift,
            )
        });
        if !down && draft.drag.take().is_some() {
            draft.sync_fields();
            self.dirty = true;
        }
        if pressed
            && !section_busy
            && draft.points.picking.is_none()
            && let Some(triad) = &triad
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && rect.contains(origin)
            && let Some((grip, dir, px_per)) = grip_at(triad, origin)
            && draft.allows(grip)
        {
            draft.drag = Some(Drag {
                grip,
                dir,
                px_per,
                start: (draft.translate, draft.turn),
            });
        }
        if let Some(drag) = draft.drag
            && down
        {
            let displacement = ui
                .input(|i| {
                    i.pointer
                        .interact_pos()
                        .zip(i.pointer.press_origin())
                        .map(|(now, start)| now - start)
                })
                .unwrap_or(Vec2::ZERO);
            let amount = f64::from(displacement.dot(drag.dir) / drag.px_per);
            // An arm snaps the total shift. A ring snaps the drag delta.
            match drag.grip {
                Grip::Arm(axis) => {
                    let mut next = drag.start.0;
                    next[axis] += amount;
                    if !shift {
                        next[axis] = snapped(next[axis], SNAP_MM);
                    }
                    draft.translate = next;
                }
                Grip::Ring(axis) => {
                    let degrees = if shift {
                        amount
                    } else {
                        snapped(amount, SNAP_DEGREES)
                    };
                    draft.turn =
                        DQuat::from_axis_angle(AXES[axis], degrees.to_radians()) * drag.start.1;
                }
            }
            draft.sync_fields();
            self.dirty = true;
        }
        if let Some(triad) = &triad
            && let Some(pos) = ui.ctx().pointer_hover_pos()
            && rect.contains(pos)
            && grip_at(triad, pos).is_some()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        draft.drag.is_some() || picked
    }

    /// Sets the picked point after a click in the viewport.
    /// Returns true when a point was set.
    fn pick_click(&mut self, ui: &egui::Ui, rect: egui::Rect) -> bool {
        let Some(stage) = self
            .move_draft
            .as_ref()
            .and_then(|draft| draft.points.picking)
        else {
            return false;
        };
        let Some(pos) = ui.input(|i| {
            if i.pointer.primary_clicked() {
                i.pointer.interact_pos()
            } else {
                None
            }
        }) else {
            return false;
        };
        if !rect.contains(pos) {
            return false;
        }
        let Some((point, found)) = self.pick_point(rect, pos, stage) else {
            return false;
        };
        let Some(draft) = self.move_draft.as_mut() else {
            return false;
        };
        // The from point is stored at the start position of the part.
        let point = match stage {
            PickStage::From => point - draft.translate,
            PickStage::To => point,
        };
        let text = point.to_array().map(mm_text);
        draft.points.note = Some(format!(
            "{found}: {}, {}, {} mm.",
            text[0], text[1], text[2]
        ));
        match stage {
            PickStage::From => draft.points.from = text,
            PickStage::To => draft.points.to = text,
        }
        draft.points.picking = None;
        self.dirty = true;
        true
    }

    /// Finds the point under a screen position. Returns the world point and how it was found.
    /// A from point is on the selected part. A to point is on another part or on the grid.
    fn pick_point(
        &self,
        rect: egui::Rect,
        pos: egui::Pos2,
        stage: PickStage,
    ) -> Option<(DVec3, &'static str)> {
        let draft = self.move_draft.as_ref()?;
        let aspect = rect.aspect_ratio();
        let matrix = self.camera.matrix(aspect);
        let ndc = [
            2. * (pos.x - rect.left()) / rect.width() - 1.,
            1. - 2. * (pos.y - rect.top()) / rect.height(),
        ];
        let (eye, ray) = self.camera.ray(aspect, ndc);
        // Parts show their preview poses, so picks follow the screen.
        let poses = self.move_poses();
        let mut surface: Option<(f64, DVec3)> = None;
        let mut vertex: Option<(f32, DVec3)> = None;
        for body in self.bodies.iter() {
            let allowed = match stage {
                PickStage::From => body.id == draft.part,
                PickStage::To => body.id != draft.part,
            };
            if !allowed || self.hidden.contains(&body.id) {
                continue;
            }
            let pose = poses
                .get(&body.id)
                .map_or(DMat4::IDENTITY, |m| m.as_dmat4());
            let inverse = pose.inverse();
            let Some(depth) = faber_scene::pick_visible(
                &body.mesh,
                inverse.transform_point3(eye),
                inverse.transform_vector3(ray),
                None,
            ) else {
                continue;
            };
            if surface.is_none_or(|(nearest, _)| depth < nearest) {
                surface = Some((depth, eye + ray * depth));
            }
            // A vertex counts when it is in front of the hit surface and near the pointer.
            for local in &body.mesh.vertices {
                let world = pose.transform_point3(DVec3::from(*local));
                let along = (world - eye).dot(ray);
                if !(0. ..=depth + SURFACE_TOLERANCE_MM).contains(&along) {
                    continue;
                }
                let Some(screen) = screen_point(rect, matrix, world) else {
                    continue;
                };
                let distance = screen.distance(pos);
                if distance <= SNAP_PX && vertex.is_none_or(|(nearest, _)| distance < nearest) {
                    vertex = Some((distance, world));
                }
            }
        }
        if let Some((_, point)) = vertex {
            return Some((point, "Vertex snap"));
        }
        let grid = (stage == PickStage::To && ray.z.abs() > 1e-12)
            .then(|| -eye.z / ray.z)
            .filter(|depth| *depth > 0.)
            .map(|depth| {
                let point = eye + ray * depth;
                let snapped = DVec3::new(
                    (point.x / SNAP_MM).round() * SNAP_MM,
                    (point.y / SNAP_MM).round() * SNAP_MM,
                    0.,
                );
                (depth, snapped)
            });
        match (surface, grid) {
            (Some((depth, _)), Some((grid_depth, grid_point))) if grid_depth < depth => {
                Some((grid_point, "Grid point"))
            }
            (Some((_, point)), _) => Some((point, "Face point")),
            (None, Some((_, grid_point))) => Some((grid_point, "Grid point")),
            (None, None) => None,
        }
    }

    /// Draws the triad over the viewport.
    pub(super) fn move_paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let Some(draft) = &self.move_draft else {
            return;
        };
        let matrix = self.camera.matrix(rect.aspect_ratio());
        let Some(triad) = triad_at(
            rect,
            matrix,
            draft.pivot_point() + draft.motion().0,
            draft.size,
        ) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        let hover = ui
            .ctx()
            .pointer_hover_pos()
            .and_then(|pos| grip_at(&triad, pos))
            .map(|(grip, _, _)| grip);
        let active = draft.drag.map(|drag| drag.grip);
        let lit = |grip: Grip| active == Some(grip) || hover == Some(grip);
        for axis in 0..3 {
            let colour = AXIS_COLOURS[axis];
            let arm_lit = lit(Grip::Arm(axis));
            let tip = triad.tips[axis];
            painter.line_segment(
                [triad.centre, tip],
                egui::Stroke::new(if arm_lit { 4. } else { 2.5 }, colour),
            );
            painter.circle_filled(tip, if arm_lit { 8. } else { 6. }, colour);
            painter.text(
                tip + triad.dirs[axis] * 14.,
                egui::Align2::CENTER_CENTER,
                ["X", "Y", "Z"][axis],
                egui::FontId::proportional(12.),
                colour,
            );
            let ring_lit = lit(Grip::Ring(axis));
            let ink = if ring_lit {
                colour
            } else {
                colour.gamma_multiply(0.6)
            };
            painter.add(egui::Shape::line(
                triad.rings[axis].clone(),
                egui::Stroke::new(if ring_lit { 3.5 } else { 2. }, ink),
            ));
        }
        painter.circle_filled(triad.centre, 3., Color32::WHITE);
        for point in draft.marks().into_iter().flatten() {
            if let Some(at) = screen_point(rect, matrix, point) {
                painter.circle_stroke(at, 6., egui::Stroke::new(2., Color32::WHITE));
                painter.circle_filled(at, 2.5, Color32::from_rgb(255, 200, 40));
            }
        }
        // Value label next to the pointer while a grip is dragged.
        if let Some(drag) = draft.drag
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            let text = match drag.grip {
                Grip::Arm(axis) => format!("{} mm", mm_text(draft.translate[axis])),
                Grip::Ring(axis) => format!("{}°", mm_text(euler_degrees(draft.turn)[axis])),
            };
            let at = egui::Rect::from_min_size(pos + Vec2::new(16., 8.), Vec2::new(72., 22.));
            painter.rect_filled(at, 4., Color32::from_black_alpha(190));
            painter.text(
                at.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(13.),
                Color32::WHITE,
            );
        }
    }

    /// Returns the poses for drawing. The moved part gets the preview turn.
    pub(super) fn move_poses(&self) -> Cow<'_, BTreeMap<String, Mat4>> {
        let Some(draft) = &self.move_draft else {
            return Cow::Borrowed(&self.motion.poses);
        };
        let pivot = draft.pivot_point().as_vec3();
        let (translate, turn) = draft.motion();
        let delta = Mat4::from_translation(pivot + translate.as_vec3())
            * Mat4::from_quat(turn.as_quat())
            * Mat4::from_translation(-pivot);
        let mut poses = self.motion.poses.clone();
        let current = poses.get(&draft.part).copied().unwrap_or(Mat4::IDENTITY);
        poses.insert(draft.part.clone(), delta * current);
        Cow::Owned(poses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use faber_model::{Feature, Geometry, Profile};

    fn same(a: DMat4, b: DMat4) -> bool {
        a.to_cols_array()
            .iter()
            .zip(b.to_cols_array())
            .all(|(x, y)| (x - y).abs() < 1e-9)
    }

    fn placement(rotation: [f64; 3], translation: [f64; 3]) -> Placement<f64> {
        Placement {
            translation,
            rotation,
        }
    }

    #[test]
    fn compose_moves_the_placement_about_the_pivot() {
        let base = placement([10., 20., 30.], [1., 2., 3.]);
        let pivot = DVec3::new(5., -4., 2.);
        let translate = DVec3::new(1., 0., -2.);
        let turn = DQuat::from_axis_angle(DVec3::Z, 0.7) * DQuat::from_axis_angle(DVec3::X, -0.4);
        let delta = DMat4::from_translation(pivot + translate)
            * DMat4::from_quat(turn)
            * DMat4::from_translation(-pivot);
        let composed = compose(base.clone(), pivot, translate, turn);
        assert!(same(
            placement_matrix(&composed),
            delta * placement_matrix(&base)
        ));
    }

    #[test]
    fn turn_about_z_from_zero_gives_z_rotation() {
        let base = placement([0., 0., 0.], [0., 0., 0.]);
        let turn = DQuat::from_axis_angle(DVec3::Z, 90f64.to_radians());
        let composed = compose(base, DVec3::ZERO, DVec3::ZERO, turn);
        for (value, expected) in composed.rotation.iter().zip([0., 0., 90.]) {
            assert!((value - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn euler_split_rebuilds_the_rotation() {
        for rotation in [
            [10., 20., 30.],
            [-45., 60., 120.],
            [170., -80., 5.],
            [0., 0., 0.],
            [-150., -30., -100.],
        ] {
            let q = placement_rotation(rotation);
            let split = euler_degrees(q);
            for i in 0..3 {
                assert!(
                    (split[i] - rotation[i]).abs() < 1e-7,
                    "{split:?} {rotation:?}"
                );
            }
        }
    }

    #[test]
    fn euler_split_handles_gimbal_lock() {
        for rotation in [[30., 90., 20.], [-70., -90., 15.]] {
            let q = placement_rotation(rotation);
            let split = euler_degrees(q);
            assert!(same(
                DMat4::from_quat(placement_rotation(split)),
                DMat4::from_quat(q)
            ));
        }
    }

    #[test]
    fn copy_id_skips_taken_names() {
        let mut parts = BTreeMap::new();
        for id in ["bin", "bin_copy"] {
            parts.insert(
                id.to_owned(),
                Part {
                    name: id.into(),
                    geometry: Geometry::Imported {
                        asset: String::new(),
                        purchased: None,
                    },
                    placement: Default::default(),
                    colour: [0; 3],
                    group: String::new(),
                    note: String::new(),
                },
            );
        }
        assert_eq!(copy_id(&parts, "bin"), "bin_copy2");
        assert_eq!(copy_id(&parts, "pin"), "pin_copy");
    }

    fn bin_project(dir: &Path) {
        let mut project = faber_wiring::open(dir, true, Some("Move test".into())).unwrap();
        let geometry = Geometry::Pocket {
            profile: Profile {
                outer: vec![[-50., -50.], [50., -50.], [50., 50.], [-50., 50.]],
                holes: vec![vec![[-46., -46.], [46., -46.], [46., 46.], [-46., 46.]]],
            },
            height: 120.,
            floor: 4.,
        };
        let part = Part {
            name: "Bin".into(),
            geometry: Geometry::History {
                features: vec![Feature {
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
    }

    fn open(dir: &Path) -> egui_kittest::Harness<'static, Shell> {
        let (data, bodies) = faber_wiring::read(dir).unwrap();
        let root = dir.to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness
    }

    fn open_move(harness: &mut egui_kittest::Harness<'_, Shell>) {
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
    }

    fn wait_for_commit(harness: &mut egui_kittest::Harness<'_, Shell>) {
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            harness.step();
            if harness.state().pending.is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "Move did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
    }

    fn saved_placement(dir: &Path, id: &str) -> Placement<f64> {
        let (data, _) = faber_wiring::read(dir).unwrap();
        let part = &data.parts[id];
        let numbers = |values: &[Scalar; 3]| {
            values.clone().map(|v| match v {
                Scalar::Number(n) => n,
                Scalar::Expression(e) => panic!("unexpected expression {e}"),
            })
        };
        Placement {
            translation: numbers(&part.placement.translation),
            rotation: numbers(&part.placement.rotation),
        }
    }

    #[test]
    fn move_menu_opens_the_tool_for_the_selected_part() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        assert_eq!(
            harness.state().move_draft.as_ref().unwrap().mode,
            MoveMode::Free
        );
        harness.get_by_label("Cancel").click();
        harness.step();
    }

    #[test]
    fn translate_field_commits_one_placement_edit() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Move").click();
        harness.step();
        harness.state_mut().move_draft.as_mut().unwrap().fields[0] = "10".into();
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        assert!(harness.state().move_draft.is_none());
        assert_eq!(
            harness.state().data.parts["bin"].placement.translation[0],
            Scalar::Number(10.)
        );
        let saved = saved_placement(dir.path(), "bin");
        assert!((saved.translation[0] - 10.).abs() < 1e-9);
        assert!(saved.translation[1].abs() < 1e-9);
    }

    #[test]
    fn rotate_z_ninety_commits_the_rotation_and_enter_confirms() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Rotate").click();
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.axis = 2;
            draft.angle = "90".into();
        }
        harness.step();
        harness.key_press(egui::Key::Enter);
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        assert!((saved.rotation[2] - 90.).abs() < 1e-9);
        assert!(saved.rotation[0].abs() < 1e-9 && saved.rotation[1].abs() < 1e-9);
    }

    #[test]
    fn create_copy_adds_a_part_and_keeps_the_source() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Create copy").click();
        harness.get_by_label("Move").click();
        harness.step();
        harness.state_mut().move_draft.as_mut().unwrap().fields[0] = "20".into();
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let (data, _) = faber_wiring::read(dir.path()).unwrap();
        assert_eq!(data.parts.len(), 2);
        assert_eq!(saved_placement(dir.path(), "bin").translation, [0., 0., 0.]);
        let copy = saved_placement(dir.path(), "bin_copy");
        assert!((copy.translation[0] - 20.).abs() < 1e-9);
        assert_eq!(data.parts["bin_copy"].name, "Bin copy");
    }

    #[test]
    fn cancel_leaves_the_project_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        let before = harness.state().data.clone();
        open_move(&mut harness);
        harness.get_by_label("Move").click();
        harness.step();
        harness.state_mut().move_draft.as_mut().unwrap().fields[0] = "15".into();
        harness.step();
        harness.get_by_label("Cancel").click();
        harness.step();
        assert!(harness.state().move_draft.is_none());
        assert_eq!(harness.state().data, before);
        assert_eq!(saved_placement(dir.path(), "bin").translation, [0., 0., 0.]);
    }

    #[test]
    fn escape_cancels_the_move() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.key_press(egui::Key::Escape);
        harness.step();
        assert!(harness.state().move_draft.is_none());
    }

    #[test]
    fn arrow_drag_moves_the_part_in_one_millimetre_steps() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        let rect = harness.get_by_label("Model viewport").rect();
        let (pivot, size) = {
            let draft = harness.state().move_draft.as_ref().unwrap();
            (draft.pivot_point(), draft.size)
        };
        let matrix = harness.state().camera.matrix(rect.aspect_ratio());
        let triad = triad_at(rect, matrix, pivot, size).unwrap();
        let press = triad.centre + (triad.tips[0] - triad.centre) * 0.75;
        let target = press + triad.dirs[0] * 40.;
        let yaw = harness.state().camera.yaw;
        harness.hover_at(press);
        harness.step();
        harness.drag_at(press);
        harness.step();
        harness.hover_at(target);
        harness.step();
        harness.drop_at(target);
        harness.step();
        let draft = harness.state().move_draft.as_ref().unwrap();
        assert!(draft.translate.x > 0.5, "{:?}", draft.translate);
        assert!((draft.translate.x - draft.translate.x.round()).abs() < 1e-9);
        assert!(draft.translate.y.abs() < 1e-9 && draft.translate.z.abs() < 1e-9);
        assert_eq!(harness.state().camera.yaw, yaw);
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    /// The bin, and a plate that floats 200 mm above the origin.
    fn plate_project(dir: &Path) {
        bin_project(dir);
        let mut project = faber_wiring::open(dir, false, None).unwrap();
        let geometry: Geometry<Scalar> = serde_json::from_value(serde_json::json!({
            "op": "history",
            "features": [{"id": "block", "geometry": {"op": "box", "size": [40, 40, 10]}}],
            "output": "block"
        }))
        .unwrap();
        let plate = Part {
            name: "Plate".into(),
            geometry,
            placement: Placement {
                translation: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(200.)],
                rotation: Default::default(),
            },
            colour: [200, 120, 80],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("plate".into(), plate).unwrap();
    }

    #[test]
    fn align_translation_uses_the_core_shift_per_axis() {
        let moving = [[0., 0., 200.], [40., 40., 210.]];
        let target = [[-10., -10., 0.], [10., 10., 120.]];
        let stacked = align_translation(
            moving,
            target,
            [false, false, true],
            AlignMode::MinToMax,
            5.,
        );
        assert_eq!(stacked, DVec3::new(0., 0., 125. - 200.));
        let centred = align_translation(moving, target, [true, true, false], AlignMode::Centre, 0.);
        assert_eq!(centred, DVec3::new(-20., -20., 0.));
        let origin =
            align_translation(moving, [[0.; 3]; 2], [true, true, true], AlignMode::Min, 0.);
        assert_eq!(origin, DVec3::new(0., 0., -200.));
    }

    #[test]
    fn point_to_point_typed_points_commit_one_placement() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Point").click();
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.points.from = ["1".into(), "2".into(), "3".into()];
            draft.points.to = ["11".into(), "2".into(), "-4".into()];
        }
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        assert!(harness.state().move_draft.is_none());
        let saved = saved_placement(dir.path(), "bin");
        assert!((saved.translation[0] - 10.).abs() < 1e-9);
        assert!(saved.translation[1].abs() < 1e-9);
        assert!((saved.translation[2] + 7.).abs() < 1e-9);
        assert!(saved.rotation.iter().all(|v| v.abs() < 1e-9));
    }

    #[test]
    fn point_to_point_cancel_leaves_the_project_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        let before = harness.state().data.clone();
        open_move(&mut harness);
        harness.get_by_label("Point").click();
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.points.to = ["30".into(), "0".into(), "0".into()];
        }
        harness.step();
        harness.get_by_label("Cancel").click();
        harness.step();
        assert!(harness.state().move_draft.is_none());
        assert_eq!(harness.state().data, before);
        assert_eq!(saved_placement(dir.path(), "bin").translation, [0., 0., 0.]);
    }

    #[test]
    fn align_stack_on_top_with_gap_saves_the_placement() {
        let dir = tempfile::tempdir().unwrap();
        plate_project(dir.path());
        let mut harness = open(dir.path());
        let (bin_top, plate_low) = {
            let bodies = &harness.state().bodies;
            let bin = bodies.iter().find(|b| b.id == "bin").unwrap();
            let plate = bodies.iter().find(|b| b.id == "plate").unwrap();
            (bin.bounds[1][2], plate.bounds[0][2])
        };
        harness.state_mut().start_move_in("plate", MoveMode::Align);
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.align.target = "bin".into();
            draft.align.axes = [false, false, true];
            draft.align.mode = AlignMode::MinToMax;
            draft.align.gap = "5".into();
        }
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "plate");
        // The plate min face sits 5 mm above the bin top.
        let expected_z = 200. + (bin_top + 5. - plate_low);
        assert!(
            (saved.translation[2] - expected_z).abs() < 1e-6,
            "{:?}",
            saved.translation
        );
        assert!(saved.translation[0].abs() < 1e-9 && saved.translation[1].abs() < 1e-9);
        assert_eq!(saved_placement(dir.path(), "bin").translation, [0., 0., 0.]);
    }

    #[test]
    fn align_refuses_a_gap_that_is_not_a_number() {
        let dir = tempfile::tempdir().unwrap();
        plate_project(dir.path());
        let mut harness = open(dir.path());
        harness.state_mut().start_move_in("plate", MoveMode::Align);
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.align.target = "bin".into();
            draft.align.gap = "wide".into();
        }
        harness.step();
        let draft = harness.state().move_draft.as_ref().unwrap();
        assert_eq!(draft.error, Some("Enter a number in mm."));
        assert_eq!(draft.translate, DVec3::ZERO);
        harness.get_by_label("Cancel").click();
        harness.step();
        assert_eq!(
            saved_placement(dir.path(), "plate").translation,
            [0., 0., 200.]
        );
    }

    #[test]
    fn vertex_snap_picks_the_nearest_mesh_vertex() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness
            .state_mut()
            .move_draft
            .as_mut()
            .unwrap()
            .points
            .picking = Some(PickStage::From);
        harness.step();
        let rect = harness.get_by_label("Model viewport").rect();
        let state = harness.state();
        let aspect = rect.aspect_ratio();
        let eye = state.camera.eye().as_dvec3();
        let body = state.bodies.iter().find(|b| b.id == "bin").unwrap();
        let vertex = body
            .mesh
            .vertices
            .iter()
            .map(|v| DVec3::from(*v))
            .min_by(|a, b| (*a - eye).length().total_cmp(&(*b - eye).length()))
            .unwrap();
        let pos = screen_point(rect, state.camera.matrix(aspect), vertex).unwrap();
        let (point, found) = state.pick_point(rect, pos, PickStage::From).unwrap();
        assert_eq!(found, "Vertex snap");
        assert!((point - vertex).length() < 1e-3, "{point:?} {vertex:?}");
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    #[test]
    fn mm_text_hides_negative_zero_and_keeps_two_decimals() {
        assert_eq!(mm_text(-0.0), "0");
        assert_eq!(mm_text(-0.004), "0");
        assert_eq!(mm_text(30.3), "30.3");
        assert_eq!(mm_text(1.2345), "1.23");
        assert_eq!(mm_text(-12.5), "-12.5");
    }

    #[test]
    fn snapped_values_are_whole_steps() {
        assert_eq!(snapped(22.4, 15.), 15.);
        assert_eq!(snapped(-7.6, 15.), -15.);
        assert_eq!(snapped(29.9, 15.), 30.);
        assert_eq!(snapped(2.6, SNAP_MM), 3.);
    }

    #[test]
    fn typed_x_distance_then_ok_saves_plus_ten() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("X distance").click();
        harness.step();
        harness.get_by_label("X distance").type_text("10");
        harness.step();
        harness.step();
        assert_eq!(
            harness.state().move_draft.as_ref().unwrap().translate.x,
            10.
        );
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        assert!((saved.translation[0] - 10.).abs() < 1e-9);
        assert!(saved.translation[1].abs() < 1e-9 && saved.translation[2].abs() < 1e-9);
    }

    #[test]
    fn ring_drag_snaps_the_turn_to_fifteen_degrees() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        let rect = harness.get_by_label("Model viewport").rect();
        let (pivot, size) = {
            let draft = harness.state().move_draft.as_ref().unwrap();
            (draft.pivot_point(), draft.size)
        };
        let matrix = harness.state().camera.matrix(rect.aspect_ratio());
        let triad = triad_at(rect, matrix, pivot, size).unwrap();
        // A point on the Z ring, away from the arms.
        let ring = &triad.rings[2];
        let index = SEGMENTS / 8;
        let press = ring[index];
        let tangent = (ring[index + 1] - ring[index - 1]).normalized();
        let target = press + tangent * 37.;
        assert!(matches!(
            grip_at(&triad, press),
            Some((Grip::Ring(2), _, _))
        ));
        harness.hover_at(press);
        harness.step();
        harness.drag_at(press);
        harness.step();
        harness.hover_at(target);
        harness.step();
        harness.drop_at(target);
        harness.step();
        let turn = euler_degrees(harness.state().move_draft.as_ref().unwrap().turn);
        assert!(turn[2].abs() > 1e-9, "{turn:?}");
        assert!(turn[0].abs() < 1e-9 && turn[1].abs() < 1e-9, "{turn:?}");
        assert!(
            (snapped(turn[2], SNAP_DEGREES) - turn[2]).abs() < 1e-9,
            "{turn:?}"
        );
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    #[test]
    fn point_to_position_moves_the_box_centre_to_the_point() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Position").click();
        harness.step();
        let centre = {
            let body = harness
                .state()
                .bodies
                .iter()
                .find(|b| b.id == "bin")
                .unwrap();
            (DVec3::from(body.bounds[0]) + DVec3::from(body.bounds[1])) / 2.
        };
        harness.state_mut().move_draft.as_mut().unwrap().position =
            ["10".into(), "20".into(), "30".into()];
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        let expected = DVec3::new(10., 20., 30.) - centre;
        for (value, want) in saved.translation.iter().zip(expected.to_array()) {
            assert!((value - want).abs() < 1e-6, "{:?}", saved.translation);
        }
    }

    #[test]
    fn pivot_origin_and_centre_give_different_z_turns() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.base.translation = [30., 0., 0.];
            draft.angles[2] = "90".into();
        }
        harness.step();
        let centre = harness.state().move_draft.as_ref().unwrap().result();
        harness.state_mut().move_draft.as_mut().unwrap().pivot_mode = PivotMode::Origin;
        harness.step();
        let origin = harness.state().move_draft.as_ref().unwrap().result();
        assert!((centre.rotation[2] - 90.).abs() < 1e-9);
        assert!((origin.rotation[2] - 90.).abs() < 1e-9);
        // The origin pivot keeps the origin in place. The centre pivot moves it.
        assert!((origin.translation[0] - 30.).abs() < 1e-9);
        assert!(origin.translation[1].abs() < 1e-9);
        assert!((centre.translation[0] - origin.translation[0]).abs() > 1.);
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    #[test]
    fn world_origin_pivot_turns_about_zero() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.base.translation = [30., 0., 0.];
            draft.angles[2] = "90".into();
            draft.pivot_mode = PivotMode::World;
        }
        harness.step();
        let saved = harness.state().move_draft.as_ref().unwrap().result();
        // Rz(90) moves (30, 0, 0) to (0, 30, 0) about the world origin.
        assert!(saved.translation[0].abs() < 1e-9, "{:?}", saved.translation);
        assert!(
            (saved.translation[1] - 30.).abs() < 1e-9,
            "{:?}",
            saved.translation
        );
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    #[test]
    fn command_panel_sits_at_the_right_of_the_viewport() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.step();
        let draft = harness.state().move_draft.as_ref().unwrap();
        let viewport = draft.viewport.unwrap();
        let panel = draft.panel_rect.unwrap();
        assert!(
            (panel.right() - viewport.right()).abs() <= PANEL_MARGIN + 4.,
            "{panel:?} {viewport:?}"
        );
        assert!(panel.left() >= viewport.left(), "{panel:?} {viewport:?}");
        assert!(panel.top() >= viewport.top() - 1., "{panel:?} {viewport:?}");
        assert!(panel.width() < viewport.width());
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    /// A screen point on a grip of the triad, and the grip direction there.
    fn grip_point(
        harness: &mut egui_kittest::Harness<'_, Shell>,
        grip: Grip,
    ) -> (egui::Pos2, Vec2) {
        let rect = harness.get_by_label("Model viewport").rect();
        let (pivot, size) = {
            let draft = harness.state().move_draft.as_ref().unwrap();
            (draft.pivot_point() + draft.translate, draft.size)
        };
        let matrix = harness.state().camera.matrix(rect.aspect_ratio());
        let triad = triad_at(rect, matrix, pivot, size).unwrap();
        let point = match grip {
            Grip::Ring(axis) => triad.rings[axis][SEGMENTS / 8],
            Grip::Arm(axis) => triad.centre + (triad.tips[axis] - triad.centre) * 0.75,
        };
        let (found, dir, _) = grip_at(&triad, point).unwrap();
        assert_eq!(found, grip);
        (point, dir)
    }

    /// Drags a grip along its direction on screen, then drops it.
    fn drag_grip(harness: &mut egui_kittest::Harness<'_, Shell>, grip: Grip, pixels: f32) {
        let (press, dir) = grip_point(harness, grip);
        let target = press + dir * pixels;
        harness.hover_at(press);
        harness.step();
        harness.drag_at(press);
        harness.step();
        harness.hover_at(target);
        harness.step();
        harness.drop_at(target);
        harness.step();
    }

    /// Drags a ring by a number of degrees. The ring uses two pixels per degree.
    fn drag_ring(harness: &mut egui_kittest::Harness<'_, Shell>, axis: usize, degrees: f32) {
        drag_grip(harness, Grip::Ring(axis), degrees * RING_PX_PER_DEGREE);
    }

    #[test]
    fn mode_change_drops_hidden_translate() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Move").click();
        harness.step();
        harness.state_mut().move_draft.as_mut().unwrap().fields[0] = "10".into();
        harness.step();
        harness.get_by_label("Rotate").click();
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.axis = 2;
            draft.angle = "90".into();
        }
        harness.step();
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        // The turn is about the bin centre (0, 0, 60). The 10 mm shift is not saved.
        assert_eq!(saved.translation, [0., 0., 0.]);
        assert_eq!(saved.rotation, [0., 0., 90.]);
    }

    #[test]
    fn two_ring_drags_save_the_previewed_orientation() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        drag_ring(&mut harness, 2, 30.);
        drag_ring(&mut harness, 1, 15.);
        let preview = harness.state().move_poses().get("bin").unwrap().as_dmat4();
        let result = placement_matrix(&harness.state().move_draft.as_ref().unwrap().result());
        assert!(preview.abs_diff_eq(result, 1e-5), "{preview:?} {result:?}");
        harness.get_by_label("OK").click();
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        // Ry(15) * Rz(30), with no Euler sum.
        let expected = DQuat::from_axis_angle(DVec3::Y, 15f64.to_radians())
            * DQuat::from_axis_angle(DVec3::Z, 30f64.to_radians());
        assert!(
            DMat3::from_quat(placement_rotation(saved.rotation))
                .abs_diff_eq(DMat3::from_quat(expected), 1e-9),
            "{:?}",
            saved.rotation
        );
        assert!(placement_matrix(&saved).abs_diff_eq(preview, 1e-4));
    }

    #[test]
    fn drags_respect_the_mode() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Move").click();
        harness.step();
        let angles = harness.state().move_draft.as_ref().unwrap().angles.clone();
        drag_ring(&mut harness, 2, 30.);
        assert_eq!(
            harness.state().move_draft.as_ref().unwrap().angles,
            angles,
            "a ring drag in Move mode changes nothing"
        );
        drag_grip(&mut harness, Grip::Arm(0), 40.);
        assert_ne!(
            harness.state().move_draft.as_ref().unwrap().fields,
            zero_text(),
            "an arrow drag in Move mode moves the part"
        );
        harness.get_by_label("Rotate").click();
        harness.step();
        let fields = harness.state().move_draft.as_ref().unwrap().fields.clone();
        drag_grip(&mut harness, Grip::Arm(0), 40.);
        assert_eq!(
            harness.state().move_draft.as_ref().unwrap().fields,
            fields,
            "an arrow drag in Rotate mode changes nothing"
        );
        harness.key_press(egui::Key::Escape);
        harness.step();
    }

    #[test]
    fn enter_reads_typed_fields_first() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Move").click();
        harness.step();
        // The text changes in this frame. Enter runs before the panel reads it.
        harness.state_mut().move_draft.as_mut().unwrap().fields[0] = "5".into();
        harness.key_press(egui::Key::Enter);
        wait_for_commit(&mut harness);
        let saved = saved_placement(dir.path(), "bin");
        assert_eq!(saved.translation[0], 5.);
    }

    #[test]
    fn saved_angles_have_no_float_noise() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut harness = open(dir.path());
        open_move(&mut harness);
        harness.get_by_label("Rotate").click();
        harness.step();
        {
            let draft = harness.state_mut().move_draft.as_mut().unwrap();
            draft.axis = 2;
            draft.angle = "90".into();
        }
        harness.step();
        harness.key_press(egui::Key::Enter);
        wait_for_commit(&mut harness);
        let (data, _) = faber_wiring::read(dir.path()).unwrap();
        assert_eq!(
            data.parts["bin"].placement.rotation,
            [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(90.)]
        );
    }

    #[test]
    fn copy_of_purchased_part_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        bin_project(dir.path());
        let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
        let hash = "f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e";
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/fastener-check/assets")
            .join(hash);
        project
            .import_with_record(
                "screw".into(),
                "Screw".into(),
                &source,
                true,
                Some(faber_model::Purchased {
                    source: "FreeCAD-library".into(),
                    licence: "CC BY 3.0".into(),
                    designation: "M3 x 10".into(),
                    mass_g: None,
                }),
            )
            .unwrap();
        drop(project);
        let mut harness = open(dir.path());
        harness.state_mut().start_move("screw");
        harness.step();
        harness.state_mut().move_draft.as_mut().unwrap().copy = true;
        harness.step();
        harness.get_by_label("OK").click();
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(60);
        while harness.state().pending.is_some() {
            harness.step();
            assert!(Instant::now() < deadline, "Move did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            harness.state().error.as_deref(),
            Some("Part screw is a purchased part. A copy is not permitted.")
        );
        assert!(harness.state().move_draft.is_some());
        let (data, _) = faber_wiring::read(dir.path()).unwrap();
        assert_eq!(data.parts.len(), 2);
    }
}
