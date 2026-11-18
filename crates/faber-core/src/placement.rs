//! Moves and aligns placed parts. The math is pure. Project methods save one change.

use crate::{Project, valid_id};
use faber_model::{Edit, Geometry, Part, Placement, ProjectData, Scalar};
use faber_ports::Result;
use glam::{DMat3, DVec3};
use schemars::JsonSchema;
use serde::Deserialize;

/// Keeps nine decimal places. Removes float noise such as 1e-17.
const DECIMALS: f64 = 1e9;
/// Sine limit. Above it, X and Z turn about one axis.
const GIMBAL_LIMIT: f64 = 1. - 1e-9;
/// Target name for a zero-size box at the world origin.
const ORIGIN: &str = "origin";

/// A world axis.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        self as usize
    }

    fn unit(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

/// How a moving part lines up with its target.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlignMode {
    /// Min faces meet.
    Min,
    /// Max faces meet.
    Max,
    /// Centres meet.
    #[serde(rename = "centre")]
    Centre,
    /// Min face of the moving part meets the max face of the target. Adds gap_mm.
    MinToMax,
    /// Max face of the moving part meets the min face of the target. Adds gap_mm.
    MaxToMin,
}

/// Rotation about an axis.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    /// Axis "x", "y" or "z", or a vector [x, y, z].
    pub axis: RotationAxis,
    /// Angle in degrees. Positive turns by the right-hand rule.
    pub angle_deg: f64,
    /// Pivot: "centre" (default), "origin" or a world point [x, y, z] in mm.
    pub about: Option<Pivot>,
}

/// Rotation axis: a named axis or a vector.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum RotationAxis {
    Named(Axis),
    Vector([f64; 3]),
}

/// Rotation pivot: a named point or a world point in mm.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum Pivot {
    Named(PivotName),
    Point([f64; 3]),
}

/// Named pivot points.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PivotName {
    /// Centre of the bounding box.
    Centre,
    /// World origin.
    Origin,
}

/// Moves a part by the vector from `from` to `to`.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PointToPoint {
    /// Start point in world mm.
    pub from: [f64; 3],
    /// End point in world mm.
    pub to: [f64; 3],
}

/// One move mode.
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    Translate([f64; 3]),
    Rotate(Rotation),
    PointToPoint(PointToPoint),
    Position([f64; 3]),
}

impl Motion {
    /// Reads the one mode that a request gives. Refuses zero or several modes.
    pub fn from_options(
        translate: Option<[f64; 3]>,
        rotate: Option<Rotation>,
        point_to_point: Option<PointToPoint>,
        position: Option<[f64; 3]>,
    ) -> Result<Self> {
        let given = [
            translate.is_some(),
            rotate.is_some(),
            point_to_point.is_some(),
            position.is_some(),
        ]
        .iter()
        .filter(|set| **set)
        .count();
        match (translate, rotate, point_to_point, position) {
            (Some(delta), None, None, None) => Ok(Self::Translate(delta)),
            (None, Some(rotation), None, None) => Ok(Self::Rotate(rotation)),
            (None, None, Some(points), None) => Ok(Self::PointToPoint(points)),
            (None, None, None, Some(point)) => Ok(Self::Position(point)),
            _ => Err(format!(
                "Give exactly one mode. Found {given}. Modes: translate, rotate, point_to_point, position."
            )),
        }
    }

    /// Returns the placement after this move. `bounds` is the world box before the move.
    pub fn apply(&self, placement: &Placement, bounds: [[f64; 3]; 2]) -> Result<Placement> {
        match self {
            Self::Translate(delta) => Ok(translated(placement, vector(*delta)?)),
            Self::Rotate(rotation) => {
                let axis = match &rotation.axis {
                    RotationAxis::Named(axis) => axis.unit(),
                    RotationAxis::Vector(axis) => vector(*axis)?.normalize_or_zero(),
                };
                if axis == DVec3::ZERO {
                    return Err("Rotation axis has zero length.".into());
                }
                if !rotation.angle_deg.is_finite() {
                    return Err("Angle must be a finite number.".into());
                }
                let pivot = match rotation.about.unwrap_or(Pivot::Named(PivotName::Centre)) {
                    Pivot::Named(PivotName::Centre) => bounds_centre(bounds),
                    Pivot::Named(PivotName::Origin) => DVec3::ZERO,
                    Pivot::Point(point) => vector(point)?,
                };
                Ok(rotated_about(placement, axis, rotation.angle_deg, pivot))
            }
            Self::PointToPoint(points) => {
                let delta = vector(points.to)? - vector(points.from)?;
                Ok(translated(placement, delta))
            }
            Self::Position(point) => {
                let delta = vector(*point)? - bounds_centre(bounds);
                Ok(translated(placement, delta))
            }
        }
    }
}

/// Moves a placement by a world vector. The rotation does not change.
pub fn translated(placement: &Placement, delta: DVec3) -> Placement {
    make(
        DVec3::from(placement.translation) + delta,
        placement.rotation,
    )
}

/// Turns a placement about a world axis and a world pivot.
/// The new rotation is the turn times the old rotation. The new translation is the turned old translation.
pub fn rotated_about(
    placement: &Placement,
    axis: DVec3,
    angle_deg: f64,
    pivot: DVec3,
) -> Placement {
    let turn = DMat3::from_axis_angle(axis, angle_deg.to_radians());
    let current = rotation_matrix(placement.rotation);
    let origin = DVec3::from(placement.translation);
    make(
        turn * (origin - pivot) + pivot,
        euler_from_matrix(turn * current),
    )
}

/// Rotation matrix of Euler angles in degrees. Rotate about X, then Y, then Z.
pub fn rotation_matrix(euler_deg: [f64; 3]) -> DMat3 {
    let [x, y, z] = euler_deg.map(f64::to_radians);
    DMat3::from_rotation_z(z) * DMat3::from_rotation_y(y) * DMat3::from_rotation_x(x)
}

/// Euler angles in degrees for a matrix from `rotation_matrix`.
/// At gimbal lock, Z is zero. Another set of angles gives the same matrix.
pub fn euler_from_matrix(matrix: DMat3) -> [f64; 3] {
    let at = |row: usize, col: usize| matrix.col(col)[row];
    let sin_y = (-at(2, 0)).clamp(-1., 1.);
    let (x, y, z) = if sin_y.abs() < GIMBAL_LIMIT {
        (
            at(2, 1).atan2(at(2, 2)),
            sin_y.asin(),
            at(1, 0).atan2(at(0, 0)),
        )
    } else {
        ((sin_y * at(0, 1)).atan2(at(1, 1)), sin_y.asin(), 0.)
    };
    [x, y, z].map(|angle| tidy(angle.to_degrees()))
}

/// Centre of a world box. `bounds` holds the min and max corners.
pub fn bounds_centre(bounds: [[f64; 3]; 2]) -> DVec3 {
    (DVec3::from(bounds[0]) + DVec3::from(bounds[1])) / 2.
}

/// Translation along one axis that lines up a moving box with a target box.
/// A positive gap adds to the direction of travel.
pub fn align_shift(
    moving: [[f64; 3]; 2],
    target: [[f64; 3]; 2],
    axis: Axis,
    mode: AlignMode,
    gap_mm: f64,
) -> f64 {
    let i = axis.index();
    let centre = |bounds: [[f64; 3]; 2]| (bounds[0][i] + bounds[1][i]) / 2.;
    match mode {
        AlignMode::Min => target[0][i] - moving[0][i],
        AlignMode::Max => target[1][i] - moving[1][i],
        AlignMode::Centre => centre(target) - centre(moving),
        AlignMode::MinToMax => (target[1][i] + gap_mm) - moving[0][i],
        AlignMode::MaxToMin => (target[0][i] - gap_mm) - moving[1][i],
    }
}

/// Reads a placement as numbers. A parameter expression is an error that names the field.
pub fn plain(id: &str, placement: &Placement<Scalar>) -> Result<Placement> {
    let read = |field: &str, values: &[Scalar; 3]| -> Result<[f64; 3]> {
        let mut out = [0.; 3];
        for (index, value) in values.iter().enumerate() {
            out[index] = match value {
                Scalar::Number(number) => *number,
                Scalar::Expression(_) => {
                    return Err(format!(
                        "{id}: placement field {field}[{index}] is a parameter expression. Replace it with a number first."
                    ));
                }
            };
        }
        Ok(out)
    };
    Ok(Placement {
        translation: read("translation", &placement.translation)?,
        rotation: read("rotation", &placement.rotation)?,
    })
}

/// Refuses a copy of a purchased part. A purchased part is a real model, so a copy must not change it.
pub fn refuse_purchased_copy(id: &str, part: &Part<Scalar>) -> Result<()> {
    match &part.geometry {
        Geometry::Imported {
            purchased: Some(_), ..
        } => Err(format!(
            "Part {id} is a purchased part. A copy is not permitted."
        )),
        _ => Ok(()),
    }
}

/// The edit that moves a part. `bounds` is the world box of the part before the move.
/// With `copy_as`, the edit adds the moved part under a new id and keeps the source.
pub fn move_edit(
    data: &ProjectData,
    id: &str,
    motion: &Motion,
    bounds: [[f64; 3]; 2],
    copy_as: Option<&str>,
) -> Result<Edit> {
    let part = part_of(data, id)?;
    let placement = motion.apply(&plain(id, &part.placement)?, bounds)?;
    placed_edit(data, id, part, placement, copy_as)
}

/// The edits that move several parts by one motion. `bounds` reads the world box of a part id.
/// The motion uses the common box of all parts, so the parts keep their spacing.
pub fn move_edits(
    data: &ProjectData,
    ids: &[String],
    motion: &Motion,
    mut bounds: impl FnMut(&str) -> Result<[[f64; 3]; 2]>,
) -> Result<Vec<Edit>> {
    if ids.is_empty() {
        return Err("Give at least one part.".into());
    }
    for (index, id) in ids.iter().enumerate() {
        if ids[..index].contains(id) {
            return Err(format!("Part listed twice: {id}"));
        }
    }
    let boxes = ids
        .iter()
        .map(|id| bounds(id))
        .collect::<Result<Vec<_>>>()?;
    let common = boxes
        .into_iter()
        .reduce(union_box)
        .ok_or_else(|| "Give at least one part.".to_string())?;
    ids.iter()
        .map(|id| {
            let part = part_of(data, id)?;
            let placement = motion.apply(&plain(id, &part.placement)?, common)?;
            placed_edit(data, id, part, placement, None)
        })
        .collect()
}

/// Smallest box that holds both boxes.
pub fn union_box(a: [[f64; 3]; 2], b: [[f64; 3]; 2]) -> [[f64; 3]; 2] {
    std::array::from_fn(|corner| {
        std::array::from_fn(|axis| {
            if corner == 0 {
                a[0][axis].min(b[0][axis])
            } else {
                a[1][axis].max(b[1][axis])
            }
        })
    })
}

/// The edit that aligns a part with a target part or with "origin". Only the listed axes move.
/// `bounds` reads the world box of a part id.
pub fn align_edit(
    data: &ProjectData,
    id: &str,
    target: &str,
    axes: &[Axis],
    mode: AlignMode,
    gap_mm: f64,
    mut bounds: impl FnMut(&str) -> Result<[[f64; 3]; 2]>,
) -> Result<Edit> {
    if axes.is_empty() {
        return Err("Give at least one axis.".into());
    }
    for (index, axis) in axes.iter().enumerate() {
        if axes[..index].contains(axis) {
            return Err(format!("Axis listed twice: {}", axis.name()));
        }
    }
    if !gap_mm.is_finite() {
        return Err("gap_mm must be a finite number.".into());
    }
    let gap_applies = matches!(mode, AlignMode::MinToMax | AlignMode::MaxToMin);
    if gap_mm != 0. && !gap_applies {
        return Err("gap_mm applies only to min_to_max and max_to_min.".into());
    }
    if target == id {
        return Err("A part cannot align to itself.".into());
    }
    let part = part_of(data, id)?;
    let moving = bounds(id)?;
    let fixed = if target == ORIGIN {
        [[0.; 3]; 2]
    } else {
        bounds(target)?
    };
    let mut delta = DVec3::ZERO;
    for axis in axes {
        delta[axis.index()] = align_shift(moving, fixed, *axis, mode, gap_mm);
    }
    let placement = translated(&plain(id, &part.placement)?, delta);
    placed_edit(data, id, part, placement, None)
}

fn part_of<'a>(data: &'a ProjectData, id: &str) -> Result<&'a Part<Scalar>> {
    data.parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))
}

/// The edit that stores a placed part. With `copy_as`, the copy gets a new id.
fn placed_edit(
    data: &ProjectData,
    id: &str,
    part: &Part<Scalar>,
    placement: Placement,
    copy_as: Option<&str>,
) -> Result<Edit> {
    let mut part = part.clone();
    part.placement = Placement {
        translation: placement.translation.map(Scalar::Number),
        rotation: placement.rotation.map(Scalar::Number),
    };
    let target = match copy_as {
        None => id,
        Some(new_id) => {
            refuse_purchased_copy(id, &part)?;
            if !valid_id(new_id) {
                return Err(format!("Invalid part id: {new_id}"));
            }
            if data.parts.contains_key(new_id) {
                return Err(format!("Part already exists: {new_id}"));
            }
            new_id
        }
    };
    Ok(Edit::PutPart {
        id: target.to_string(),
        part,
    })
}

fn vector(values: [f64; 3]) -> Result<DVec3> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(DVec3::from(values))
    } else {
        Err("Values must be finite numbers.".into())
    }
}

/// Builds a placement with tidy numbers.
fn make(translation: DVec3, euler_deg: [f64; 3]) -> Placement {
    Placement {
        translation: translation.to_array().map(tidy),
        rotation: euler_deg.map(tidy),
    }
}

/// Rounds to nine decimal places. The `+ 0.0` turns negative zero into zero.
pub fn tidy(value: f64) -> f64 {
    (value * DECIMALS).round() / DECIMALS + 0.0
}

impl Project {
    /// Moves a part by one mode. With `copy_as`, keeps the source and adds the moved copy.
    /// The change is one undo step.
    pub fn move_part(&mut self, id: &str, motion: Motion, copy_as: Option<&str>) -> Result<()> {
        self.sync()?;
        let bounds = self.world_bounds(id)?;
        let edit = move_edit(&self.data, id, &motion, bounds, copy_as)?;
        self.batch(vec![edit])
    }

    /// Moves several parts by one motion. The change is one undo step.
    pub fn move_parts(&mut self, ids: &[String], motion: Motion) -> Result<()> {
        self.sync()?;
        let edits = move_edits(&self.data, ids, &motion, |part| self.world_bounds(part))?;
        self.batch(edits)
    }

    /// Aligns a part with a target part or with "origin". Only the listed axes move.
    /// The change is one undo step.
    pub fn align_part(
        &mut self,
        id: &str,
        target: &str,
        axes: &[Axis],
        mode: AlignMode,
        gap_mm: f64,
    ) -> Result<()> {
        self.sync()?;
        let edit = align_edit(&self.data, id, target, axes, mode, gap_mm, |part| {
            self.world_bounds(part)
        })?;
        self.batch(vec![edit])
    }

    fn world_bounds(&self, id: &str) -> Result<[[f64; 3]; 2]> {
        self.bodies
            .iter()
            .find(|body| body.id == id)
            .map(|body| body.bounds)
            .ok_or_else(|| format!("Unknown part: {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: [[f64; 3]; 2] = [[0., 0., 0.], [10., 20., 30.]];

    fn identity() -> Placement {
        Placement {
            translation: [0.; 3],
            rotation: [0.; 3],
        }
    }

    fn apply_point(placement: &Placement, point: DVec3) -> DVec3 {
        rotation_matrix(placement.rotation) * point + DVec3::from(placement.translation)
    }

    #[test]
    fn translate_adds_to_translation_only() {
        let start = Placement {
            translation: [1., 2., 3.],
            rotation: [10., 20., 30.],
        };
        let moved = Motion::Translate([4., -5., 6.]).apply(&start, BOX).unwrap();
        assert_eq!(moved.translation, [5., -3., 9.]);
        assert_eq!(moved.rotation, start.rotation);
    }

    #[test]
    fn compose_matches_turn_about_pivot() {
        let start = Placement {
            translation: [1., 2., 3.],
            rotation: [10., 20., 30.],
        };
        let axis = DVec3::new(0.3, -0.5, 0.8).normalize();
        let pivot = DVec3::new(4., 5., 6.);
        let turned = rotated_about(&start, axis, 37., pivot);
        let turn = DMat3::from_axis_angle(axis, 37f64.to_radians());
        for point in [DVec3::ZERO, DVec3::X, DVec3::new(-7., 3., 11.)] {
            let before = apply_point(&start, point);
            let expected = turn * (before - pivot) + pivot;
            assert!(apply_point(&turned, point).abs_diff_eq(expected, 1e-9));
        }
    }

    #[test]
    fn euler_round_trip_keeps_the_matrix() {
        for x in (-170..=170).step_by(37) {
            for y in (-80..=80).step_by(23) {
                for z in (-170..=170).step_by(41) {
                    let euler = [f64::from(x), f64::from(y), f64::from(z)];
                    let back = euler_from_matrix(rotation_matrix(euler));
                    for (a, b) in back.iter().zip(euler) {
                        assert!((a - b).abs() < 1e-7, "{euler:?} gave {back:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn gimbal_lock_keeps_the_matrix() {
        for euler in [[30., 90., 0.], [-20., -90., 15.], [45., 90., 60.]] {
            let matrix = rotation_matrix(euler);
            let back = euler_from_matrix(matrix);
            assert_eq!(back[2], 0.);
            assert!((back[1].abs() - 90.).abs() < 1e-7, "{back:?}");
            let again = rotation_matrix(back);
            assert!(matrix.abs_diff_eq(again, 1e-9), "{euler:?} gave {back:?}");
        }
    }

    #[test]
    fn right_angle_turn_has_no_float_noise() {
        let turned = rotated_about(&identity(), DVec3::Z, 90., DVec3::new(5., 10., 0.));
        assert_eq!(turned.rotation, [0., 0., 90.]);
        assert_eq!(turned.translation, [15., 5., 0.]);
    }

    #[test]
    fn centre_pivot_uses_bounds_centre() {
        let moved = Motion::Rotate(Rotation {
            axis: RotationAxis::Named(Axis::Z),
            angle_deg: 90.,
            about: None,
        })
        .apply(&identity(), BOX)
        .unwrap();
        let expected = rotated_about(&identity(), DVec3::Z, 90., DVec3::new(5., 10., 15.));
        assert_eq!(moved, expected);
        assert_eq!(moved.rotation, [0., 0., 90.]);
    }

    #[test]
    fn position_moves_bounds_centre_to_point() {
        let moved = Motion::Position([100., 0., 0.])
            .apply(&identity(), BOX)
            .unwrap();
        assert_eq!(moved.translation, [95., -10., -15.]);
    }

    #[test]
    fn point_to_point_moves_by_the_vector() {
        let moved = Motion::PointToPoint(PointToPoint {
            from: [1., 1., 1.],
            to: [4., 5., 1.],
        })
        .apply(&identity(), BOX)
        .unwrap();
        assert_eq!(moved.translation, [3., 4., 0.]);
    }

    #[test]
    fn modes_must_be_exactly_one() {
        assert!(Motion::from_options(None, None, None, None).is_err());
        let two = Motion::from_options(Some([1., 0., 0.]), None, None, Some([0., 0., 0.]));
        assert!(two.unwrap_err().contains("Found 2"));
        assert!(Motion::from_options(Some([1., 0., 0.]), None, None, None).is_ok());
    }

    #[test]
    fn expression_field_is_named() {
        let placement = Placement {
            translation: [
                Scalar::Number(0.),
                Scalar::Expression("L".into()),
                Scalar::Number(0.),
            ],
            rotation: [Scalar::Number(0.), Scalar::Number(0.), Scalar::Number(0.)],
        };
        let error = plain("bar", &placement).unwrap_err();
        assert!(error.contains("translation[1]"), "{error}");
    }

    #[test]
    fn min_to_max_stacks_with_gap() {
        let base = [[0., 0., 0.], [40., 40., 10.]];
        let top = [[0., 0., 3.], [10., 10., 8.]];
        let shift = align_shift(top, base, Axis::Z, AlignMode::MinToMax, 2.);
        assert_eq!(shift, 9.);
    }

    #[test]
    fn max_to_min_uses_gap_towards_the_target() {
        let target = [[0., 0., 0.], [40., 40., 10.]];
        let moving = [[0., 0., 20.], [10., 10., 25.]];
        let shift = align_shift(moving, target, Axis::Z, AlignMode::MaxToMin, 3.);
        assert_eq!(shift, -28.);
    }

    #[test]
    fn centre_and_origin_modes() {
        let target = [[0., 0., 0.], [40., 40., 10.]];
        let moving = [[10., 10., 5.], [20., 30., 15.]];
        assert_eq!(
            align_shift(moving, target, Axis::X, AlignMode::Centre, 0.),
            5.
        );
        assert_eq!(
            align_shift(moving, target, Axis::Y, AlignMode::Min, 0.),
            -10.
        );
        let origin = [[0.; 3]; 2];
        assert_eq!(
            align_shift(moving, origin, Axis::Z, AlignMode::Max, 0.),
            -15.
        );
    }

    #[test]
    fn tidy_removes_float_noise_and_negative_zero() {
        assert_eq!(tidy(90.00000000000001), 90.);
        assert!(tidy(-1e-17).is_sign_positive());
        assert_eq!(tidy(12.5), 12.5);
    }
}
