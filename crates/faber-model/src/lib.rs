//! Project recipes. Lengths are in millimetres and angles are in degrees.

mod material;
mod printer;
pub use printer::{Clearance, Printer};
pub mod motion;
pub mod study;
pub use material::{MaterialClass, MaterialData};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    SetPrinter {
        printer: Printer,
    },
    SetStudy {
        id: String,
        study: study::Study<Scalar>,
    },
    RemoveStudy {
        id: String,
    },
    SetMotion {
        study: motion::MotionStudy,
    },
    SetParam {
        name: String,
        expression: String,
    },
    PutPart {
        id: String,
        part: Part<Scalar>,
    },
    RemovePart {
        id: String,
    },
    AddFeature {
        part: String,
        feature: Feature<Scalar>,
        before: Option<String>,
        #[serde(default)]
        make_output: bool,
    },
    EditFeature {
        part: String,
        id: String,
        geometry: Geometry<Scalar>,
    },
    DeleteFeature {
        part: String,
        id: String,
    },
    MoveFeature {
        part: String,
        id: String,
        before: Option<String>,
    },
    /// Suppresses or unsuppresses a feature. A suppressed feature is skipped in the build.
    /// With cascade, suppressing also suppresses the features that depend on it.
    SetSuppressed {
        part: String,
        id: String,
        suppressed: bool,
        #[serde(default)]
        cascade: bool,
    },
    /// Declares, or with allowed false removes, an intended contact of two parts.
    SetContact {
        a: String,
        b: String,
        allowed: bool,
        #[serde(default)]
        note: String,
        /// Deepest allowed overlap in millimetres. Absent means the default.
        #[serde(default)]
        max_depth_mm: Option<f64>,
    },
    /// Adds a saved section analysis, or replaces the one with the same id.
    SetSection {
        id: String,
        section: SectionAnalysis<Scalar>,
    },
    /// Removes a saved section analysis.
    RemoveSection {
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct Profile<N = f64> {
    pub outer: Vec<[N; 2]>,
    #[serde(default)]
    pub holes: Vec<Vec<[N; 2]>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Purchased {
    pub source: String,
    pub licence: String,
    pub designation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mass_g: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "N: Deserialize<'de> + Default"))]
#[schemars(bound = "N: JsonSchema + Default")]
pub enum Geometry<N = f64> {
    Imported {
        asset: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purchased: Option<Box<Purchased>>,
    },
    Mirror {
        source: String,
        origin: [N; 3],
        normal: [N; 3],
        #[serde(default)]
        keep_source: bool,
    },
    CircularPattern {
        source: String,
        count: N,
        origin: [N; 3],
        axis: [N; 3],
        angle: N,
    },
    LinearPattern {
        source: String,
        count: N,
        step: [N; 3],
    },
    /// Copies of a solid along a path. Copy 0 is the source.
    PathPattern {
        source: String,
        path: PathInput<N>,
        count: N,
        /// Distance between copies along the path. Absent spaces the copies evenly over the whole path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing: Option<N>,
        /// Turn each copy to follow the path tangent.
        #[serde(default)]
        orient: bool,
    },
    Curve {
        path: CurvePath<N>,
    },
    Helix {
        radius: N,
        pitch: N,
        turns: N,
        #[serde(default)]
        placement: Placement<N>,
    },
    SweepSketch {
        #[serde(default)]
        frenet: bool,
        source: String,
        path: String,
    },
    RevolveSketch {
        source: String,
        origin: [N; 3],
        axis: [N; 3],
        angle: N,
    },
    LoftSketches {
        sources: Vec<String>,
        #[serde(default)]
        ruled: bool,
    },
    Sketch {
        profile: SketchProfile<N>,
        #[serde(default)]
        placement: Placement<N>,
    },
    Plane {
        placement: Placement<N>,
    },
    PlaneOnFace {
        source: String,
        face: FaceSelector<N>,
        offset: N,
    },
    Axis {
        origin: [N; 3],
        direction: [N; 3],
    },
    Point {
        position: [N; 3],
    },
    /// A plane through a reference axis. It turns `angle` degrees about the axis from `from`.
    PlaneAtAngle {
        axis: String,
        from: AngleFrom,
        angle: N,
    },
    /// The plane halfway between two parallel reference planes.
    Midplane {
        first: String,
        second: String,
    },
    /// The plane through three points. Each point is a point feature or coordinates.
    PlaneThroughPoints {
        points: [PointInput<N>; 3],
    },
    /// The axis of a cylindrical face selected on the source.
    AxisFromCylinder {
        source: String,
        face: FaceSelector<N>,
    },
    /// The line where two reference planes cross.
    AxisFromPlanes {
        first: String,
        second: String,
    },
    /// The centre of a full circular edge selected on the source.
    PointAtCentre {
        source: String,
        edge: EdgeSelector<N>,
    },
    RevolveAround {
        source: String,
        axis: String,
        angle: N,
    },
    PlaceAtPoint {
        source: String,
        point: String,
    },
    SketchOnPlane {
        plane: String,
        profile: SketchProfile<N>,
        #[serde(default)]
        placement: Placement<N>,
    },
    ExtrudeSketch {
        source: String,
        vector: [N; 3],
    },
    Shell {
        source: String,
        faces: FaceSelector<N>,
        thickness: N,
        /// Thickness for faces that differ from `thickness`. Each face needs one rule.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thickness_overrides: Vec<ShellThickness<N>>,
    },
    Offset {
        source: String,
        distance: N,
    },
    Thread {
        source: String,
        face: FaceSelector<N>,
        size: String,
        length: N,
        #[serde(default)]
        offset: N,
        #[serde(default)]
        left_handed: bool,
    },
    Hole {
        source: String,
        diameter: N,
        depth: N,
        #[serde(default)]
        placement: Placement<N>,
        head: Option<HoleHead<N>>,
    },
    Split {
        source: String,
        origin: [N; 3],
        normal: [N; 3],
        #[serde(default)]
        keep: SplitKeep,
    },
    Thicken {
        source: String,
        thickness: N,
        face: Option<FaceSelector<N>>,
    },
    Draft {
        source: String,
        faces: FaceSelector<N>,
        direction: [N; 3],
        origin: [N; 3],
        normal: [N; 3],
        angle: N,
    },
    Fillet {
        source: String,
        edges: EdgeSelector<N>,
        radius: N,
        /// Radius at the end of each edge. The radius changes linearly from `radius`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_end: Option<N>,
    },
    Chamfer {
        source: String,
        edges: EdgeSelector<N>,
        distance: N,
        /// Second distance. Cannot be set with `angle_deg`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<N>,
        /// Angle in degrees from the face. Cannot be set with `distance2`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle_deg: Option<N>,
        /// Face where `distance` is measured. Default: the first face of each edge.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceSelector<N>>,
    },
    Sweep {
        profile: Profile<N>,
        #[serde(default)]
        placement: Placement<N>,
        path: Vec<[N; 3]>,
    },
    /// A round bar along a path. A wall makes a tube.
    Pipe {
        path: PathInput<N>,
        diameter: N,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wall: Option<N>,
    },
    Loft {
        sections: Vec<LoftSection<N>>,
        #[serde(default)]
        ruled: bool,
    },
    Sphere {
        radius: N,
    },
    Cone {
        base_radius: N,
        top_radius: N,
        height: N,
    },
    Torus {
        major_radius: N,
        minor_radius: N,
    },
    Revolve {
        profile: Profile<N>,
        origin: [N; 3],
        axis: [N; 3],
        angle: N,
    },
    Box {
        size: [N; 3],
    },
    Cylinder {
        radius: N,
        height: N,
    },
    Transform {
        source: String,
        placement: Placement<N>,
    },
    /// Scales a solid about a point in part coordinates.
    Scale {
        source: String,
        factor: ScaleFactor<N>,
        about: [N; 3],
    },
    Union {
        left: String,
        right: String,
    },
    Difference {
        left: String,
        right: String,
    },
    Intersection {
        left: String,
        right: String,
    },
    History {
        features: Vec<Feature<N>>,
        output: String,
    },
    Extrude {
        profile: Profile<N>,
        height: N,
    },
    Pocket {
        profile: Profile<N>,
        height: N,
        floor: N,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "N: Deserialize<'de> + Default")
)]
#[schemars(bound = "N: JsonSchema + Default")]
pub struct Feature<N = f64> {
    pub id: String,
    pub geometry: Geometry<N>,
    /// A suppressed feature is skipped in the build. It stays in the recipe.
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppressed: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
#[schemars(bound = "N: JsonSchema + Default")]
pub struct LoftSection<N = f64> {
    pub profile: Profile<N>,
    #[serde(default)]
    pub placement: Placement<N>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EdgeSelector<N = f64> {
    pub between: Option<[FaceSelector<N>; 2]>,
    pub of_face: Option<FaceSelector<N>>,
    pub parallel: Option<[N; 3]>,
    pub near_centre: Option<[N; 3]>,
    pub in_box: Option<[[N; 3]; 2]>,
    pub length: Option<N>,
    pub radius: Option<N>,
    pub tolerance: N,
    pub expect: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FaceSelector<N = f64> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cylinder_radius: Option<N>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal: Option<bool>,
    pub created_by: Option<String>,
    pub normal: Option<[N; 3]>,
    pub near_centre: Option<[N; 3]>,
    pub in_box: Option<[[N; 3]; 2]>,
    pub area: Option<N>,
    pub tolerance: N,
    pub expect: usize,
}

/// Wall thickness for the faces that match `faces` in a shell.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShellThickness<N = f64> {
    pub faces: FaceSelector<N>,
    pub thickness: N,
}

impl<N> ShellThickness<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<ShellThickness<T>, String> {
        Ok(ShellThickness {
            faces: self.faces.map_numbers(map)?,
            thickness: map(&self.thickness)?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoleHead<N = f64> {
    Counterbore { diameter: N, depth: N },
    Countersink { diameter: N, angle: N },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitKeep {
    #[default]
    Both,
    Positive,
    Negative,
}

impl<N> HoleHead<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<HoleHead<T>, String> {
        Ok(match self {
            Self::Counterbore { diameter, depth } => HoleHead::Counterbore {
                diameter: map(diameter)?,
                depth: map(depth)?,
            },
            Self::Countersink { diameter, angle } => HoleHead::Countersink {
                diameter: map(diameter)?,
                angle: map(angle)?,
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchLoop<N = f64> {
    Grid {
        item: Box<SketchLoop<N>>,
        count: [usize; 2],
        step: [[N; 2]; 2],
    },
    Slot {
        centre: [N; 2],
        length: N,
        width: N,
        rotation: N,
    },
    Ellipse {
        centre: [N; 2],
        major_radius: N,
        minor_radius: N,
        rotation: N,
    },
    RegularPolygon {
        centre: [N; 2],
        radius: N,
        sides: usize,
        rotation: N,
        /// Radius of each rounded corner. Absent or zero keeps sharp corners.
        #[serde(skip_serializing_if = "Option::is_none")]
        corner_radius: Option<N>,
    },
    /// Rectangle with rounded corners. Each corner has the same radius.
    RoundedRectangle {
        centre: [N; 2],
        width: N,
        height: N,
        radius: N,
        rotation: N,
    },
    Path {
        start: [N; 2],
        segments: Vec<SketchSegment<N>>,
    },
    Circle {
        centre: [N; 2],
        radius: N,
    },
    Rectangle {
        origin: [N; 2],
        size: [N; 2],
    },
    Polygon {
        points: Vec<[N; 2]>,
        /// Radius of each rounded corner. Absent or zero keeps sharp corners.
        #[serde(skip_serializing_if = "Option::is_none")]
        corner_radius: Option<N>,
    },
    /// One line of text. Only the outer boundary of a sketch can be text.
    /// `height` is the cap height. `at` is the baseline start, or the centre
    /// when `align` is centre. `angle_deg` turns the text about `at`.
    Text {
        text: String,
        height: N,
        at: [N; 2],
        #[serde(default)]
        align: TextAlign,
        /// Turn about `at` in degrees. Absent means zero.
        #[serde(skip_serializing_if = "Option::is_none")]
        angle_deg: Option<N>,
        /// Built-in font name. Absent uses `ubuntu-light`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font: Option<String>,
    },
}

/// Horizontal text alignment about the `at` point.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Centre,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchSegment<N = f64> {
    Spline {
        points: Vec<[N; 2]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tangents: Option<[[N; 2]; 2]>,
        #[serde(default)]
        periodic: bool,
    },
    Line {
        end: [N; 2],
    },
    Arc {
        via: [N; 2],
        end: [N; 2],
    },
    Bezier {
        control1: [N; 2],
        control2: [N; 2],
        end: [N; 2],
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CurvePath<N = f64> {
    pub start: [N; 3],
    pub segments: Vec<CurveSegment<N>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveSegment<N = f64> {
    Spline {
        points: Vec<[N; 3]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tangents: Option<[[N; 3]; 2]>,
        #[serde(default)]
        periodic: bool,
    },
    Line {
        end: [N; 3],
    },
    Arc {
        via: [N; 3],
        end: [N; 3],
    },
    Bezier {
        control1: [N; 3],
        control2: [N; 3],
        end: [N; 3],
    },
}

/// One factor for all axes, or one factor for each axis.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
#[schemars(bound = "N: JsonSchema + Default")]
pub enum ScaleFactor<N = f64> {
    Uniform(N),
    Axes([N; 3]),
}

impl<N> ScaleFactor<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<ScaleFactor<T>, String> {
        Ok(match self {
            Self::Uniform(value) => ScaleFactor::Uniform(map(value)?),
            Self::Axes([x, y, z]) => ScaleFactor::Axes([map(x)?, map(y)?, map(z)?]),
        })
    }
}

/// A path for `pipe` and `path_pattern`. A feature id names a `curve` or `helix`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
#[schemars(bound = "N: JsonSchema + Default")]
pub enum PathInput<N = f64> {
    Feature(String),
    Curve(CurvePath<N>),
}

/// The reference plane for `plane_at_angle`. The base planes are XY, XZ and YZ.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AngleFrom {
    Xy,
    Xz,
    Yz,
    /// A plane feature id. The axis must lie in this plane.
    Plane(String),
}

/// A point for `plane_through_points`: a point feature id or coordinates.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
#[schemars(bound = "N: JsonSchema + Default")]
pub enum PointInput<N = f64> {
    Feature(String),
    Position([N; 3]),
}

impl<N> PointInput<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<PointInput<T>, String> {
        Ok(match self {
            Self::Feature(name) => PointInput::Feature(name.clone()),
            Self::Position([x, y, z]) => PointInput::Position([map(x)?, map(y)?, map(z)?]),
        })
    }
}

impl<N> PathInput<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<PathInput<T>, String> {
        Ok(match self {
            Self::Feature(name) => PathInput::Feature(name.clone()),
            Self::Curve(path) => PathInput::Curve(path.map_numbers(map)?),
        })
    }
}

impl<N> CurvePath<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<CurvePath<T>, String> {
        let mut point = |[x, y, z]: &[N; 3]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]);
        let start = point(&self.start)?;
        let segments = self
            .segments
            .iter()
            .map(|s| {
                Ok(match s {
                    CurveSegment::Spline {
                        points,
                        tangents,
                        periodic,
                    } => CurveSegment::Spline {
                        periodic: *periodic,
                        points: points
                            .iter()
                            .map(&mut point)
                            .collect::<Result<_, String>>()?,
                        tangents: tangents
                            .as_ref()
                            .map(|[a, b]| Ok::<_, String>([point(a)?, point(b)?]))
                            .transpose()?,
                    },
                    CurveSegment::Line { end } => CurveSegment::Line { end: point(end)? },
                    CurveSegment::Arc { via, end } => CurveSegment::Arc {
                        via: point(via)?,
                        end: point(end)?,
                    },
                    CurveSegment::Bezier {
                        control1,
                        control2,
                        end,
                    } => CurveSegment::Bezier {
                        control1: point(control1)?,
                        control2: point(control2)?,
                        end: point(end)?,
                    },
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(CurvePath { start, segments })
    }
}

impl<N> SketchSegment<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SketchSegment<T>, String> {
        let mut point = |[x, y]: &[N; 2]| Ok::<_, String>([map(x)?, map(y)?]);
        Ok(match self {
            Self::Spline {
                points,
                tangents,
                periodic,
            } => SketchSegment::Spline {
                periodic: *periodic,
                points: points
                    .iter()
                    .map(&mut point)
                    .collect::<Result<_, String>>()?,
                tangents: tangents
                    .as_ref()
                    .map(|[a, b]| Ok::<_, String>([point(a)?, point(b)?]))
                    .transpose()?,
            },
            Self::Line { end } => SketchSegment::Line { end: point(end)? },
            Self::Arc { via, end } => SketchSegment::Arc {
                via: point(via)?,
                end: point(end)?,
            },
            Self::Bezier {
                control1,
                control2,
                end,
            } => SketchSegment::Bezier {
                control1: point(control1)?,
                control2: point(control2)?,
                end: point(end)?,
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct SketchProfile<N = f64> {
    pub outer: SketchLoop<N>,
    #[serde(default)]
    pub holes: Vec<SketchLoop<N>>,
}

impl<N> SketchLoop<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SketchLoop<T>, String> {
        Ok(match self {
            Self::Grid { item, count, step } => SketchLoop::Grid {
                item: Box::new(item.map_numbers(map)?),
                count: *count,
                step: [
                    [map(&step[0][0])?, map(&step[0][1])?],
                    [map(&step[1][0])?, map(&step[1][1])?],
                ],
            },
            Self::Slot {
                centre: [x, y],
                length,
                width,
                rotation,
            } => SketchLoop::Slot {
                centre: [map(x)?, map(y)?],
                length: map(length)?,
                width: map(width)?,
                rotation: map(rotation)?,
            },
            Self::Ellipse {
                centre: [x, y],
                major_radius,
                minor_radius,
                rotation,
            } => SketchLoop::Ellipse {
                centre: [map(x)?, map(y)?],
                major_radius: map(major_radius)?,
                minor_radius: map(minor_radius)?,
                rotation: map(rotation)?,
            },
            Self::RegularPolygon {
                centre: [x, y],
                radius,
                sides,
                rotation,
                corner_radius,
            } => SketchLoop::RegularPolygon {
                centre: [map(x)?, map(y)?],
                radius: map(radius)?,
                sides: *sides,
                rotation: map(rotation)?,
                corner_radius: corner_radius.as_ref().map(&mut *map).transpose()?,
            },
            Self::RoundedRectangle {
                centre: [x, y],
                width,
                height,
                radius,
                rotation,
            } => SketchLoop::RoundedRectangle {
                centre: [map(x)?, map(y)?],
                width: map(width)?,
                height: map(height)?,
                radius: map(radius)?,
                rotation: map(rotation)?,
            },
            Self::Path {
                start: [x, y],
                segments,
            } => SketchLoop::Path {
                start: [map(x)?, map(y)?],
                segments: segments
                    .iter()
                    .map(|s| s.map_numbers(map))
                    .collect::<Result<_, String>>()?,
            },
            Self::Circle {
                centre: [x, y],
                radius,
            } => SketchLoop::Circle {
                centre: [map(x)?, map(y)?],
                radius: map(radius)?,
            },
            Self::Rectangle {
                origin: [x, y],
                size: [w, h],
            } => SketchLoop::Rectangle {
                origin: [map(x)?, map(y)?],
                size: [map(w)?, map(h)?],
            },
            Self::Polygon {
                points,
                corner_radius,
            } => SketchLoop::Polygon {
                points: points
                    .iter()
                    .map(|[x, y]| Ok([map(x)?, map(y)?]))
                    .collect::<Result<_, String>>()?,
                corner_radius: corner_radius.as_ref().map(&mut *map).transpose()?,
            },
            Self::Text {
                text,
                height,
                at: [x, y],
                align,
                angle_deg,
                font,
            } => SketchLoop::Text {
                text: text.clone(),
                height: map(height)?,
                at: [map(x)?, map(y)?],
                align: *align,
                angle_deg: angle_deg.as_ref().map(&mut *map).transpose()?,
                font: font.clone(),
            },
        })
    }
}

impl<N> SketchProfile<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SketchProfile<T>, String> {
        Ok(SketchProfile {
            outer: self.outer.map_numbers(map)?,
            holes: self
                .holes
                .iter()
                .map(|hole| hole.map_numbers(map))
                .collect::<Result<_, String>>()?,
        })
    }
}

fn map_box<N, T>(
    value: &Option<[[N; 3]; 2]>,
    map: &mut impl FnMut(&N) -> Result<T, String>,
) -> Result<Option<[[T; 3]; 2]>, String> {
    value
        .as_ref()
        .map(|[min, max]| {
            let mut point = |[x, y, z]: &[N; 3]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]);
            Ok::<_, String>([point(min)?, point(max)?])
        })
        .transpose()
}

impl<N> FaceSelector<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<FaceSelector<T>, String> {
        Ok(FaceSelector {
            cylinder_radius: self.cylinder_radius.as_ref().map(&mut *map).transpose()?,
            internal: self.internal,
            created_by: self.created_by.clone(),
            normal: self
                .normal
                .as_ref()
                .map(|[x, y, z]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]))
                .transpose()?,
            near_centre: self
                .near_centre
                .as_ref()
                .map(|[x, y, z]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]))
                .transpose()?,
            in_box: map_box(&self.in_box, map)?,
            area: self.area.as_ref().map(&mut *map).transpose()?,
            tolerance: map(&self.tolerance)?,
            expect: self.expect,
        })
    }
}

impl<N> EdgeSelector<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<EdgeSelector<T>, String> {
        Ok(EdgeSelector {
            between: self
                .between
                .as_ref()
                .map(|[a, b]| Ok::<_, String>([a.map_numbers(map)?, b.map_numbers(map)?]))
                .transpose()?,
            of_face: self
                .of_face
                .as_ref()
                .map(|face| face.map_numbers(map))
                .transpose()?,
            parallel: self
                .parallel
                .as_ref()
                .map(|[x, y, z]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]))
                .transpose()?,
            near_centre: self
                .near_centre
                .as_ref()
                .map(|[x, y, z]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]))
                .transpose()?,
            in_box: map_box(&self.in_box, map)?,
            length: self.length.as_ref().map(&mut *map).transpose()?,
            radius: self.radius.as_ref().map(&mut *map).transpose()?,
            tolerance: map(&self.tolerance)?,
            expect: self.expect,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
#[schemars(bound = "N: JsonSchema + Default")]
pub struct Placement<N = f64> {
    #[serde(default)]
    pub translation: [N; 3],
    /// Rotate about X, then Y, then Z.
    #[serde(default)]
    pub rotation: [N; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
#[schemars(bound = "N: JsonSchema + Default")]
pub struct Part<N = f64> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub name: String,
    pub geometry: Geometry<N>,
    #[serde(default)]
    pub placement: Placement<N>,
    pub colour: [u8; 3],
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<Printer>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub studies: BTreeMap<String, study::Study<Scalar>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub materials: BTreeMap<String, MaterialData>,
    pub schema: u32,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "motion_empty")]
    pub motion: motion::MotionStudy,
    pub name: String,
    pub parts: BTreeMap<String, Part<Scalar>>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Intended contacts, keyed by `a|b` with a sorted before b.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contacts: BTreeMap<String, Contact>,
    /// Saved section analyses, keyed by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, SectionAnalysis<Scalar>>,
}

fn motion_empty(study: &motion::MotionStudy) -> bool {
    study.joints.is_empty() && study.gears.is_empty() && study.linkages.is_empty()
}

/// A pair of parts that can touch or press on purpose. Parts a and b are sorted.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub a: String,
    pub b: String,
    #[serde(default)]
    pub note: String,
    /// Deepest allowed overlap in millimetres. Touch is always allowed.
    #[serde(
        default = "default_contact_depth_mm",
        skip_serializing_if = "is_default_contact_depth_mm"
    )]
    pub max_depth_mm: f64,
}

/// Default depth limit of a declared contact, in millimetres.
pub const DEFAULT_CONTACT_DEPTH_MM: f64 = 0.6;

fn default_contact_depth_mm() -> f64 {
    DEFAULT_CONTACT_DEPTH_MM
}

fn is_default_contact_depth_mm(value: &f64) -> bool {
    *value == DEFAULT_CONTACT_DEPTH_MM
}

/// A saved section analysis. It changes views only, never solids.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "N: Deserialize<'de> + Default")
)]
#[schemars(bound = "N: JsonSchema + Default")]
pub struct SectionAnalysis<N = f64> {
    pub plane: SectionPlane<N>,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Part ids to cut. Empty cuts all parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(deserialize = "N: Deserialize<'de> + Default")
)]
#[schemars(bound = "N: JsonSchema + Default")]
pub enum SectionPlane<N = f64> {
    /// Origin and normal in world coordinates.
    Plane { origin: [N; 3], normal: [N; 3] },
    /// A `plane` reference feature of a part, plus a signed offset along its local Z.
    Reference {
        part: String,
        feature: String,
        #[serde(default)]
        offset: N,
    },
    /// One planar face of a part, found by a face selector, plus a signed offset along its outward normal.
    Face {
        part: String,
        #[serde(default)]
        feature: Option<String>,
        face: FaceSelector<N>,
        #[serde(default)]
        offset: N,
    },
}

impl<N> SectionAnalysis<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SectionAnalysis<T>, String> {
        Ok(SectionAnalysis {
            plane: self.plane.map_numbers(map)?,
            reverse: self.reverse,
            visible: self.visible,
            parts: self.parts.clone(),
            note: self.note.clone(),
        })
    }
}

impl<N> SectionPlane<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SectionPlane<T>, String> {
        Ok(match self {
            Self::Plane { origin, normal } => {
                let [ox, oy, oz] = origin;
                let [nx, ny, nz] = normal;
                SectionPlane::Plane {
                    origin: [map(ox)?, map(oy)?, map(oz)?],
                    normal: [map(nx)?, map(ny)?, map(nz)?],
                }
            }
            Self::Reference {
                part,
                feature,
                offset,
            } => SectionPlane::Reference {
                part: part.clone(),
                feature: feature.clone(),
                offset: map(offset)?,
            },
            Self::Face {
                part,
                feature,
                face,
                offset,
            } => SectionPlane::Face {
                part: part.clone(),
                feature: feature.clone(),
                face: face.map_numbers(map)?,
                offset: map(offset)?,
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum Scalar {
    Number(f64),
    Expression(String),
}

impl Default for Scalar {
    fn default() -> Self {
        Self::Number(0.)
    }
}

impl From<f64> for Scalar {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl<N: Clone> Part<N> {
    pub fn map_numbers<T>(
        &self,
        mut map: impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Part<T>, String> {
        Ok(Part {
            material: self.material.clone(),
            name: self.name.clone(),
            geometry: self.geometry.map_numbers(&mut map)?,
            placement: self.placement.map_numbers(&mut map)?,
            colour: self.colour,
            group: self.group.clone(),
            note: self.note.clone(),
        })
    }
}

impl<N> Profile<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Profile<T>, String> {
        let mut ring = |points: &[[N; 2]]| -> Result<Vec<[T; 2]>, String> {
            points
                .iter()
                .map(|p| Ok([map(&p[0])?, map(&p[1])?]))
                .collect()
        };
        Ok(Profile {
            outer: ring(&self.outer)?,
            holes: self
                .holes
                .iter()
                .map(|p| ring(p))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl<N> Geometry<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Geometry<T>, String> {
        Ok(match self {
            Geometry::RevolveSketch {
                source,
                origin: [x, y, z],
                axis: [a, b, c],
                angle,
            } => Geometry::RevolveSketch {
                source: source.clone(),
                origin: [map(x)?, map(y)?, map(z)?],
                axis: [map(a)?, map(b)?, map(c)?],
                angle: map(angle)?,
            },
            Geometry::LoftSketches { sources, ruled } => Geometry::LoftSketches {
                sources: sources.clone(),
                ruled: *ruled,
            },
            Geometry::Helix {
                radius,
                pitch,
                turns,
                placement,
            } => Geometry::Helix {
                radius: map(radius)?,
                pitch: map(pitch)?,
                turns: map(turns)?,
                placement: placement.map_numbers(map)?,
            },
            Geometry::Axis { origin, direction } => Geometry::Axis {
                origin: [map(&origin[0])?, map(&origin[1])?, map(&origin[2])?],
                direction: [
                    map(&direction[0])?,
                    map(&direction[1])?,
                    map(&direction[2])?,
                ],
            },
            Geometry::Point { position } => Geometry::Point {
                position: [map(&position[0])?, map(&position[1])?, map(&position[2])?],
            },
            Geometry::PlaneAtAngle { axis, from, angle } => Geometry::PlaneAtAngle {
                axis: axis.clone(),
                from: from.clone(),
                angle: map(angle)?,
            },
            Geometry::Midplane { first, second } => Geometry::Midplane {
                first: first.clone(),
                second: second.clone(),
            },
            Geometry::PlaneThroughPoints { points } => Geometry::PlaneThroughPoints {
                points: [
                    points[0].map_numbers(map)?,
                    points[1].map_numbers(map)?,
                    points[2].map_numbers(map)?,
                ],
            },
            Geometry::AxisFromCylinder { source, face } => Geometry::AxisFromCylinder {
                source: source.clone(),
                face: face.map_numbers(map)?,
            },
            Geometry::AxisFromPlanes { first, second } => Geometry::AxisFromPlanes {
                first: first.clone(),
                second: second.clone(),
            },
            Geometry::PointAtCentre { source, edge } => Geometry::PointAtCentre {
                source: source.clone(),
                edge: edge.map_numbers(map)?,
            },
            Geometry::RevolveAround {
                source,
                axis,
                angle,
            } => Geometry::RevolveAround {
                source: source.clone(),
                axis: axis.clone(),
                angle: map(angle)?,
            },
            Geometry::PlaceAtPoint { source, point } => Geometry::PlaceAtPoint {
                source: source.clone(),
                point: point.clone(),
            },
            Geometry::PlaneOnFace {
                source,
                face,
                offset,
            } => Geometry::PlaneOnFace {
                source: source.clone(),
                face: face.map_numbers(map)?,
                offset: map(offset)?,
            },
            Geometry::Plane { placement } => Geometry::Plane {
                placement: placement.map_numbers(map)?,
            },
            Geometry::SketchOnPlane {
                plane,
                profile,
                placement,
            } => Geometry::SketchOnPlane {
                plane: plane.clone(),
                profile: profile.map_numbers(map)?,
                placement: placement.map_numbers(map)?,
            },
            Geometry::Curve { path } => Geometry::Curve {
                path: path.map_numbers(map)?,
            },
            Geometry::SweepSketch {
                source,
                path,
                frenet,
            } => Geometry::SweepSketch {
                frenet: *frenet,
                source: source.clone(),
                path: path.clone(),
            },
            Geometry::Sketch { profile, placement } => Geometry::Sketch {
                profile: profile.map_numbers(map)?,
                placement: placement.map_numbers(map)?,
            },
            Geometry::ExtrudeSketch {
                source,
                vector: [x, y, z],
            } => Geometry::ExtrudeSketch {
                source: source.clone(),
                vector: [map(x)?, map(y)?, map(z)?],
            },
            Geometry::Draft {
                source,
                faces,
                direction,
                origin,
                normal,
                angle,
            } => Geometry::Draft {
                source: source.clone(),
                faces: faces.map_numbers(map)?,
                direction: [
                    map(&direction[0])?,
                    map(&direction[1])?,
                    map(&direction[2])?,
                ],
                origin: [map(&origin[0])?, map(&origin[1])?, map(&origin[2])?],
                normal: [map(&normal[0])?, map(&normal[1])?, map(&normal[2])?],
                angle: map(angle)?,
            },
            Geometry::Split {
                source,
                origin,
                normal,
                keep,
            } => Geometry::Split {
                source: source.clone(),
                origin: [map(&origin[0])?, map(&origin[1])?, map(&origin[2])?],
                normal: [map(&normal[0])?, map(&normal[1])?, map(&normal[2])?],
                keep: *keep,
            },
            Geometry::Thicken {
                source,
                thickness,
                face,
            } => Geometry::Thicken {
                source: source.clone(),
                thickness: map(thickness)?,
                face: face
                    .as_ref()
                    .map(|face| face.map_numbers(map))
                    .transpose()?,
            },
            Geometry::Thread {
                source,
                face,
                size,
                length,
                offset,
                left_handed,
            } => Geometry::Thread {
                source: source.clone(),
                face: face.map_numbers(map)?,
                size: size.clone(),
                length: map(length)?,
                offset: map(offset)?,
                left_handed: *left_handed,
            },
            Geometry::Hole {
                source,
                diameter,
                depth,
                placement,
                head,
            } => Geometry::Hole {
                source: source.clone(),
                diameter: map(diameter)?,
                depth: map(depth)?,
                placement: placement.map_numbers(map)?,
                head: head
                    .as_ref()
                    .map(|head| head.map_numbers(map))
                    .transpose()?,
            },
            Geometry::Offset { source, distance } => Geometry::Offset {
                source: source.clone(),
                distance: map(distance)?,
            },
            Geometry::Shell {
                source,
                faces,
                thickness,
                thickness_overrides,
            } => Geometry::Shell {
                source: source.clone(),
                faces: faces.map_numbers(map)?,
                thickness: map(thickness)?,
                thickness_overrides: thickness_overrides
                    .iter()
                    .map(|item| item.map_numbers(map))
                    .collect::<Result<_, _>>()?,
            },
            Geometry::Fillet {
                source,
                edges,
                radius,
                radius_end,
            } => Geometry::Fillet {
                source: source.clone(),
                edges: edges.map_numbers(map)?,
                radius: map(radius)?,
                radius_end: radius_end.as_ref().map(&mut *map).transpose()?,
            },
            Geometry::Chamfer {
                source,
                edges,
                distance,
                distance2,
                angle_deg,
                face,
            } => Geometry::Chamfer {
                source: source.clone(),
                edges: edges.map_numbers(map)?,
                distance: map(distance)?,
                distance2: distance2.as_ref().map(&mut *map).transpose()?,
                angle_deg: angle_deg.as_ref().map(&mut *map).transpose()?,
                face: face
                    .as_ref()
                    .map(|value| value.map_numbers(map))
                    .transpose()?,
            },
            Geometry::Sweep {
                profile,
                placement,
                path,
            } => Geometry::Sweep {
                profile: profile.map_numbers(map)?,
                placement: placement.map_numbers(map)?,
                path: path
                    .iter()
                    .map(|[x, y, z]| Ok([map(x)?, map(y)?, map(z)?]))
                    .collect::<Result<_, String>>()?,
            },
            Geometry::Loft { sections, ruled } => Geometry::Loft {
                sections: sections
                    .iter()
                    .map(|section| {
                        Ok(LoftSection {
                            profile: section.profile.map_numbers(map)?,
                            placement: section.placement.map_numbers(map)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
                ruled: *ruled,
            },
            Geometry::Sphere { radius } => Geometry::Sphere {
                radius: map(radius)?,
            },
            Geometry::Cone {
                base_radius,
                top_radius,
                height,
            } => Geometry::Cone {
                base_radius: map(base_radius)?,
                top_radius: map(top_radius)?,
                height: map(height)?,
            },
            Geometry::Torus {
                major_radius,
                minor_radius,
            } => Geometry::Torus {
                major_radius: map(major_radius)?,
                minor_radius: map(minor_radius)?,
            },
            Geometry::Revolve {
                profile: p,
                origin: [x, y, z],
                axis: [a, b, c],
                angle,
            } => Geometry::Revolve {
                profile: p.map_numbers(map)?,
                origin: [map(x)?, map(y)?, map(z)?],
                axis: [map(a)?, map(b)?, map(c)?],
                angle: map(angle)?,
            },
            Geometry::Box { size: [x, y, z] } => Geometry::Box {
                size: [map(x)?, map(y)?, map(z)?],
            },
            Geometry::Cylinder { radius, height } => Geometry::Cylinder {
                radius: map(radius)?,
                height: map(height)?,
            },
            Geometry::Imported { asset, purchased } => Geometry::Imported {
                purchased: purchased.clone(),
                asset: asset.clone(),
            },
            Geometry::Mirror {
                source,
                origin: [x, y, z],
                normal: [a, b, c],
                keep_source,
            } => Geometry::Mirror {
                source: source.clone(),
                origin: [map(x)?, map(y)?, map(z)?],
                normal: [map(a)?, map(b)?, map(c)?],
                keep_source: *keep_source,
            },
            Geometry::CircularPattern {
                source,
                count,
                origin: [x, y, z],
                axis: [a, b, c],
                angle,
            } => Geometry::CircularPattern {
                source: source.clone(),
                count: map(count)?,
                origin: [map(x)?, map(y)?, map(z)?],
                axis: [map(a)?, map(b)?, map(c)?],
                angle: map(angle)?,
            },
            Geometry::LinearPattern {
                source,
                count,
                step: [x, y, z],
            } => Geometry::LinearPattern {
                source: source.clone(),
                count: map(count)?,
                step: [map(x)?, map(y)?, map(z)?],
            },
            Geometry::Transform { source, placement } => Geometry::Transform {
                source: source.clone(),
                placement: placement.map_numbers(map)?,
            },
            Geometry::Scale {
                source,
                factor,
                about: [x, y, z],
            } => Geometry::Scale {
                source: source.clone(),
                factor: factor.map_numbers(map)?,
                about: [map(x)?, map(y)?, map(z)?],
            },
            Geometry::Pipe {
                path,
                diameter,
                wall,
            } => Geometry::Pipe {
                path: path.map_numbers(map)?,
                diameter: map(diameter)?,
                wall: wall.as_ref().map(&mut *map).transpose()?,
            },
            Geometry::PathPattern {
                source,
                path,
                count,
                spacing,
                orient,
            } => Geometry::PathPattern {
                source: source.clone(),
                path: path.map_numbers(map)?,
                count: map(count)?,
                spacing: spacing.as_ref().map(&mut *map).transpose()?,
                orient: *orient,
            },
            Geometry::Union { left, right } => Geometry::Union {
                left: left.clone(),
                right: right.clone(),
            },
            Geometry::Difference { left, right } => Geometry::Difference {
                left: left.clone(),
                right: right.clone(),
            },
            Geometry::Intersection { left, right } => Geometry::Intersection {
                left: left.clone(),
                right: right.clone(),
            },
            Geometry::History { features, output } => Geometry::History {
                features: features
                    .iter()
                    .map(|feature| {
                        Ok(Feature {
                            id: feature.id.clone(),
                            geometry: feature.geometry.map_numbers(map)?,
                            suppressed: feature.suppressed,
                        })
                    })
                    .collect::<Result<_, String>>()?,
                output: output.clone(),
            },
            Geometry::Extrude { profile: p, height } => Geometry::Extrude {
                profile: p.map_numbers(map)?,
                height: map(height)?,
            },
            Geometry::Pocket {
                profile: p,
                height,
                floor,
            } => Geometry::Pocket {
                profile: p.map_numbers(map)?,
                height: map(height)?,
                floor: map(floor)?,
            },
        })
    }
}

impl<N> Placement<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Placement<T>, String> {
        let [x, y, z] = &self.translation;
        let translation = [map(x)?, map(y)?, map(z)?];
        let [x, y, z] = &self.rotation;
        let rotation = [map(x)?, map(y)?, map(z)?];
        Ok(Placement {
            translation,
            rotation,
        })
    }
}
