use crate::construction;
use crate::feature_cache;
use crate::valid_id;
use faber_model::{AngleFrom, Feature, Geometry, PathInput, PointInput};
use faber_ports::{Combine, FeatureCache, Kernel, Result, Shape};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests {
    use super::*;
    use faber_ports::{Mesh, Properties};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counter(AtomicUsize);
    impl Kernel for Counter {
        fn build(&self, _: &Geometry) -> Result<Mesh> {
            unreachable!()
        }
        fn exact_shape(&self, geometry: &Geometry) -> Result<Option<Shape>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            let Geometry::Box { size } = geometry else {
                unreachable!()
            };
            Ok(Some(Shape::new(size.iter().product::<f64>())))
        }
        fn combine(&self, a: &Shape, b: &Shape, _: Combine) -> Result<Shape> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Shape::new(
                a.get::<f64>().unwrap() + b.get::<f64>().unwrap(),
            ))
        }
        fn properties(&self, shape: &Shape) -> Result<Properties> {
            Ok(Properties {
                volume: *shape.get::<f64>().unwrap(),
                centre: [0.; 3],
                bounds: [[0.; 3]; 2],
                faces: 6,
                edges: 12,
                solids: 1,
                valid: true,
            })
        }
    }

    #[test]
    fn creator_selection_refuses_a_kernel_without_face_history() {
        let kernel = Counter(AtomicUsize::new(0));
        let features = vec![
            Feature {
                id: "base".into(),
                geometry: Geometry::Box { size: [10.; 3] },
                suppressed: false,
            },
            Feature {
                id: "plane".into(),
                geometry: Geometry::PlaneOnFace {
                    source: "base".into(),
                    offset: 0.,
                    face: faber_model::FaceSelector {
                        cylinder_radius: None,
                        internal: None,
                        created_by: Some("base".into()),
                        normal: Some([0., 0., 1.]),
                        near_centre: None,
                        in_box: None,
                        area: None,
                        tolerance: 1e-6,
                        expect: 1,
                    },
                },
                suppressed: false,
            },
        ];
        let error = build(
            &features,
            "plane",
            &kernel,
            &mut Cache::default(),
            &Default::default(),
            None,
        )
        .unwrap_err();
        assert!(error.contains("Face history is not available"), "{error}");
    }

    #[test]
    fn changes_rebuild_only_the_feature_and_its_dependants() {
        let kernel = Counter(AtomicUsize::new(0));
        let mut features = vec![
            Feature {
                id: "a".into(),
                geometry: Geometry::Box { size: [10.; 3] },
                suppressed: false,
            },
            Feature {
                id: "b".into(),
                geometry: Geometry::Box { size: [20.; 3] },
                suppressed: false,
            },
            Feature {
                id: "sum".into(),
                geometry: Geometry::Union {
                    left: "a".into(),
                    right: "b".into(),
                },
                suppressed: false,
            },
        ];
        let mut cache = Cache::default();
        build(
            &features,
            "sum",
            &kernel,
            &mut cache,
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(kernel.0.load(Ordering::Relaxed), 3);
        features[0].geometry = Geometry::Box { size: [11.; 3] };
        build(
            &features,
            "sum",
            &kernel,
            &mut cache,
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(kernel.0.load(Ordering::Relaxed), 5);
        build(
            &features,
            "sum",
            &kernel,
            &mut cache,
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(kernel.0.load(Ordering::Relaxed), 5);
    }

    #[derive(Default)]
    struct Memory(std::sync::Mutex<BTreeMap<String, Vec<u8>>>);
    impl FeatureCache for Memory {
        fn get(&self, key: &str) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
            self.0.lock().unwrap().insert(key.into(), bytes.to_vec());
            Ok(())
        }
    }

    struct Versioned {
        version: &'static str,
        builds: AtomicUsize,
    }
    impl Kernel for Versioned {
        fn build(&self, _: &Geometry) -> Result<Mesh> {
            unreachable!()
        }
        fn exact_shape(&self, geometry: &Geometry) -> Result<Option<Shape>> {
            self.builds.fetch_add(1, Ordering::Relaxed);
            let Geometry::Box { size } = geometry else {
                unreachable!()
            };
            Ok(Some(Shape::new(size.iter().product::<f64>())))
        }
        fn properties(&self, shape: &Shape) -> Result<Properties> {
            Ok(Properties {
                volume: *shape.get::<f64>().unwrap(),
                centre: [0.; 3],
                bounds: [[0.; 3]; 2],
                faces: 6,
                edges: 12,
                solids: 1,
                valid: true,
            })
        }
        fn cache_version(&self) -> Option<&'static str> {
            Some(self.version)
        }
        fn encode_shape(&self, shape: &Shape) -> Result<Vec<u8>> {
            Ok(shape.get::<f64>().unwrap().to_le_bytes().to_vec())
        }
        fn decode_shape(&self, bytes: &[u8]) -> Result<Shape> {
            let bytes: [u8; 8] = bytes.try_into().map_err(|_| "Bad shape bytes".to_owned())?;
            Ok(Shape::new(f64::from_le_bytes(bytes)))
        }
        fn face_count(&self, _: &Shape) -> Result<u32> {
            Ok(6)
        }
    }

    #[test]
    fn persistent_entries_depend_on_the_kernel_version() {
        let store = Memory::default();
        let features = vec![Feature {
            id: "base".into(),
            geometry: Geometry::Box { size: [10.; 3] },
            suppressed: false,
        }];
        let first = Versioned {
            version: "v1",
            builds: AtomicUsize::new(0),
        };
        build(
            &features,
            "base",
            &first,
            &mut Cache::default(),
            &Default::default(),
            Some(&store),
        )
        .unwrap();
        assert_eq!(first.builds.load(Ordering::Relaxed), 1);

        let same = Versioned {
            version: "v1",
            builds: AtomicUsize::new(0),
        };
        let (shape, _) = build(
            &features,
            "base",
            &same,
            &mut Cache::default(),
            &Default::default(),
            Some(&store),
        )
        .unwrap();
        assert_eq!(
            same.builds.load(Ordering::Relaxed),
            0,
            "same version built again"
        );
        assert_eq!(shape.get::<f64>(), Some(&1000.));

        let other = Versioned {
            version: "v2",
            builds: AtomicUsize::new(0),
        };
        build(
            &features,
            "base",
            &other,
            &mut Cache::default(),
            &Default::default(),
            Some(&store),
        )
        .unwrap();
        assert_eq!(
            other.builds.load(Ordering::Relaxed),
            1,
            "old version entry was used"
        );
    }
}

#[derive(Clone, Default)]
pub(crate) struct Cache(BTreeMap<String, Entry>);

impl Cache {
    pub(crate) fn shape(&self, id: &str) -> Option<&Shape> {
        self.0.get(id).and_then(|entry| match &entry.value {
            Built::Shape(shape) => Some(shape),
            _ => None,
        })
    }
    /// The frame of a reference plane feature, in part coordinates.
    pub(crate) fn placement(&self, id: &str) -> Option<faber_model::Placement> {
        self.0.get(id).and_then(|entry| match &entry.value {
            Built::Plane(placement) => Some(placement.clone()),
            _ => None,
        })
    }
    /// The origin and direction of a reference axis feature, in part coordinates.
    pub(crate) fn axis(&self, id: &str) -> Option<([f64; 3], [f64; 3])> {
        self.0.get(id).and_then(|entry| match &entry.value {
            Built::Axis { origin, direction } => Some((*origin, *direction)),
            _ => None,
        })
    }
    /// The position of a reference point feature, in part coordinates.
    pub(crate) fn point(&self, id: &str) -> Option<[f64; 3]> {
        self.0.get(id).and_then(|entry| match &entry.value {
            Built::Point(position) => Some(*position),
            _ => None,
        })
    }
}

#[derive(Clone)]
enum Built {
    Shape(Shape),
    Plane(faber_model::Placement),
    Axis {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Point([f64; 3]),
}

impl Built {
    fn same_input(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Shape(a), Self::Shape(b)) => a.same_instance(b),
            (Self::Plane(a), Self::Plane(b)) => a == b,
            (Self::Point(a), Self::Point(b)) => a == b,
            (
                Self::Axis {
                    origin: a,
                    direction: b,
                },
                Self::Axis {
                    origin: c,
                    direction: d,
                },
            ) => a == c && b == d,
            _ => false,
        }
    }
}

/// The reference or shape inputs of a construction reference, in the order that `construction_value` reads them.
fn construction_inputs(
    geometry: &Geometry,
    shapes: &BTreeMap<String, Built>,
) -> Result<Option<Vec<Built>>> {
    let get = |name: &str| {
        shapes
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Unknown previous feature: {name}"))
    };
    let axis = |name: &str| match get(name)? {
        value @ Built::Axis { .. } => Ok(value),
        _ => Err(format!("Feature {name} is not a reference axis")),
    };
    let plane = |name: &str| match get(name)? {
        value @ Built::Plane(_) => Ok(value),
        _ => Err(format!("Feature {name} is not a reference plane")),
    };
    let point = |name: &str| match get(name)? {
        value @ Built::Point(_) => Ok(value),
        _ => Err(format!("Feature {name} is not a reference point")),
    };
    let shape = |name: &str| match get(name)? {
        value @ Built::Shape(_) => Ok(value),
        _ => Err(format!("Feature {name} is a reference, not a shape")),
    };
    Ok(Some(match geometry {
        Geometry::PlaneAtAngle {
            axis: name, from, ..
        } => {
            let mut inputs = vec![axis(name)?];
            if let AngleFrom::Plane(plane_name) = from {
                inputs.push(plane(plane_name)?);
            }
            inputs
        }
        Geometry::Midplane { first, second } | Geometry::AxisFromPlanes { first, second } => {
            vec![plane(first)?, plane(second)?]
        }
        Geometry::PlaneThroughPoints { points } => points
            .iter()
            .filter_map(|point_input| match point_input {
                PointInput::Feature(name) => Some(point(name)),
                PointInput::Position(_) => None,
            })
            .collect::<Result<_>>()?,
        Geometry::AxisFromCylinder { source, .. } | Geometry::PointAtCentre { source, .. } => {
            vec![shape(source)?]
        }
        _ => return Ok(None),
    }))
}

/// The value of a construction reference from its inputs. A failure names the operation only.
fn construction_value(kernel: &dyn Kernel, geometry: &Geometry, inputs: &[Built]) -> Result<Built> {
    let mismatch = || Err("Reference inputs do not match the operation".to_owned());
    match (geometry, inputs) {
        (
            Geometry::PlaneAtAngle { from, angle, .. },
            [Built::Axis { origin, direction }, rest @ ..],
        ) => {
            let reference = match (from, rest) {
                (AngleFrom::Xy, []) => (glam::DVec3::ZERO, glam::DVec3::Z),
                (AngleFrom::Xz, []) => (glam::DVec3::ZERO, glam::DVec3::Y),
                (AngleFrom::Yz, []) => (glam::DVec3::ZERO, glam::DVec3::X),
                (AngleFrom::Plane(_), [Built::Plane(plane)]) => construction::plane_of(plane),
                _ => return mismatch(),
            };
            construction::angled((*origin, *direction), reference, *angle).map(Built::Plane)
        }
        (Geometry::Midplane { .. }, [Built::Plane(first), Built::Plane(second)]) => {
            construction::midplane(first, second).map(Built::Plane)
        }
        (Geometry::AxisFromPlanes { .. }, [Built::Plane(first), Built::Plane(second)]) => {
            let (origin, direction) = construction::plane_axis(
                construction::plane_of(first),
                construction::plane_of(second),
            )?;
            Ok(Built::Axis { origin, direction })
        }
        (Geometry::PlaneThroughPoints { points }, inputs) => {
            let mut found = inputs.iter();
            let mut positions = [glam::DVec3::ZERO; 3];
            for (slot, point_input) in positions.iter_mut().zip(points) {
                *slot = match point_input {
                    PointInput::Feature(_) => match found.next() {
                        Some(Built::Point(position)) => glam::DVec3::from(*position),
                        _ => return mismatch(),
                    },
                    PointInput::Position(position) => glam::DVec3::from(*position),
                };
            }
            construction::through_points(positions).map(Built::Plane)
        }
        (Geometry::AxisFromCylinder { face, .. }, [Built::Shape(shape)]) => {
            let (origin, direction) = construction::cylinder_axis(kernel, shape, face)?;
            Ok(Built::Axis { origin, direction })
        }
        (Geometry::PointAtCentre { edge, .. }, [Built::Shape(shape)]) => {
            construction::circle_centre(kernel, shape, edge).map(Built::Point)
        }
        _ => mismatch(),
    }
}

fn reference(geometry: &Geometry) -> Result<Option<Built>> {
    let coordinates = |values: &[f64]| {
        if values.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
            Err("Reference values must be finite with absolute value at most 1000000".to_owned())
        } else {
            Ok(())
        }
    };
    Ok(Some(match geometry {
        Geometry::Plane { placement } => {
            coordinates(&placement.translation)?;
            coordinates(&placement.rotation)?;
            Built::Plane(placement.clone())
        }
        Geometry::Axis { origin, direction } => {
            coordinates(origin)?;
            coordinates(direction)?;
            if direction.iter().map(|v| v * v).sum::<f64>() < 1e-20 {
                return Err("Reference axis needs a non-zero direction".into());
            }
            Built::Axis {
                origin: *origin,
                direction: *direction,
            }
        }
        Geometry::Point { position } => {
            coordinates(position)?;
            Built::Point(*position)
        }
        _ => return Ok(None),
    }))
}

#[derive(Clone)]
struct Entry {
    geometry: Geometry,
    dependencies: Vec<Built>,
    value: Built,
}

const SHELL_SIGN_RULE: &str =
    "Shell thickness must be positive. The shell grows inward from the outer faces.";
const SHELL_CONFLICT: &str =
    "A face has more than one shell rule. Each face can be removed or get one thickness.";
const SHELL_HINT: &str = "Shell before you fillet or chamfer the edges that touch the removed faces. A fillet radius smaller than the thickness can also fail.";

fn not_positive(value: f64) -> bool {
    value.is_nan() || value <= 0.
}

fn dress_with_hint(size: f64, mut operation: impl FnMut(f64) -> Result<Shape>) -> Result<Shape> {
    let failure = match operation(size) {
        Ok(shape) => return Ok(shape),
        Err(error) => error,
    };
    if !size.is_finite() || size <= 0. {
        return Err(failure);
    }
    let mut low = 0.;
    let mut high = size;
    for _ in 0..5 {
        let candidate = (low + high) / 2.;
        match operation(candidate) {
            Ok(_) => low = candidate,
            Err(_) => high = candidate,
        }
    }
    if low > 0. {
        Err(format!(
            "{failure}; largest tested value that works: {low} mm. No change was saved"
        ))
    } else {
        Err(format!(
            "{failure}; no tested smaller value works. No change was saved"
        ))
    }
}

/// Names a feature failure. The text gets the feature id and the operation kind.
/// A text that already names the feature gets the kind added.
pub(crate) fn named(feature: &Feature, error: String) -> String {
    let id = &feature.id;
    let kind = operation_kind(&feature.geometry);
    if let Some(rest) = error.strip_prefix(&format!("Feature {id}: ")) {
        return format!("Feature {id} ({kind}): {rest}");
    }
    if error.starts_with(&format!("Feature {id} (")) {
        return error;
    }
    format!("Feature {id} ({kind}): {error}")
}

/// Names the output feature of a history part before the text of a later failure.
pub(crate) fn output_error(features: &[Feature], output: &str, error: String) -> String {
    match features.iter().find(|feature| feature.id == output) {
        Some(feature) => named(feature, error),
        None => error,
    }
}

/// Returns the operation kind as a snake case name, such as `fillet`.
pub(crate) fn operation_kind<N>(geometry: &Geometry<N>) -> &'static str {
    match geometry {
        Geometry::Instance { .. } => "instance",
        Geometry::Imported { .. } => "imported",
        Geometry::Rib { .. } => "rib",
        Geometry::Web { .. } => "web",
        Geometry::Boss { .. } => "boss",
        Geometry::SnapHook { .. } => "snap_hook",
        Geometry::Lip { .. } => "lip",
        Geometry::Groove { .. } => "groove",
        Geometry::Mirror { .. } => "mirror",
        Geometry::CircularPattern { .. } => "circular_pattern",
        Geometry::LinearPattern { .. } => "linear_pattern",
        Geometry::PathPattern { .. } => "path_pattern",
        Geometry::Curve { .. } => "curve",
        Geometry::Helix { .. } => "helix",
        Geometry::SweepSketch { .. } => "sweep_sketch",
        Geometry::RevolveSketch { .. } => "revolve_sketch",
        Geometry::LoftSketches { .. } => "loft_sketches",
        Geometry::Sketch { .. } => "sketch",
        Geometry::Plane { .. } => "plane",
        Geometry::PlaneOnFace { .. } => "plane_on_face",
        Geometry::Axis { .. } => "axis",
        Geometry::Point { .. } => "point",
        Geometry::PlaneAtAngle { .. } => "plane_at_angle",
        Geometry::Midplane { .. } => "midplane",
        Geometry::PlaneThroughPoints { .. } => "plane_through_points",
        Geometry::AxisFromCylinder { .. } => "axis_from_cylinder",
        Geometry::AxisFromPlanes { .. } => "axis_from_planes",
        Geometry::PointAtCentre { .. } => "point_at_centre",
        Geometry::RevolveAround { .. } => "revolve_around",
        Geometry::PlaceAtPoint { .. } => "place_at_point",
        Geometry::SketchOnPlane { .. } => "sketch_on_plane",
        Geometry::ExtrudeSketch { .. } => "extrude_sketch",
        Geometry::Shell { .. } => "shell",
        Geometry::Offset { .. } => "offset",
        Geometry::Thread { .. } => "thread",
        Geometry::Hole { .. } => "hole",
        Geometry::Split { .. } => "split",
        Geometry::Thicken { .. } => "thicken",
        Geometry::Draft { .. } => "draft",
        Geometry::Fillet { .. } => "fillet",
        Geometry::Chamfer { .. } => "chamfer",
        Geometry::Sweep { .. } => "sweep",
        Geometry::Pipe { .. } => "pipe",
        Geometry::Loft { .. } => "loft",
        Geometry::Sphere { .. } => "sphere",
        Geometry::Cone { .. } => "cone",
        Geometry::Torus { .. } => "torus",
        Geometry::Revolve { .. } => "revolve",
        Geometry::Box { .. } => "box",
        Geometry::Cylinder { .. } => "cylinder",
        Geometry::Transform { .. } => "transform",
        Geometry::Scale { .. } => "scale",
        Geometry::Union { .. } => "union",
        Geometry::Difference { .. } => "difference",
        Geometry::Intersection { .. } => "intersection",
        Geometry::History { .. } => "history",
        Geometry::Extrude { .. } => "extrude",
        Geometry::Pocket { .. } => "pocket",
    }
}

/// Names of the features that a feature reads. The order matches `dependencies`.
pub(crate) fn references<N>(geometry: &Geometry<N>) -> Vec<&str> {
    let names: Vec<&String> = match geometry {
        Geometry::Rib { body, plane, .. }
        | Geometry::Web { body, plane, .. }
        | Geometry::Boss { body, plane, .. }
        | Geometry::SnapHook { body, plane, .. } => vec![body, plane],
        Geometry::Lip { body, .. } | Geometry::Groove { body, .. } => vec![body],
        Geometry::SketchOnPlane { plane, .. } => vec![plane],
        Geometry::PlaneAtAngle { axis, from, .. } => match from {
            AngleFrom::Plane(plane) => vec![axis, plane],
            _ => vec![axis],
        },
        Geometry::Midplane { first, second } | Geometry::AxisFromPlanes { first, second } => {
            vec![first, second]
        }
        Geometry::PlaneThroughPoints { points } => points
            .iter()
            .filter_map(|point| match point {
                PointInput::Feature(name) => Some(name),
                PointInput::Position(_) => None,
            })
            .collect(),
        Geometry::AxisFromCylinder { source, .. } | Geometry::PointAtCentre { source, .. } => {
            vec![source]
        }
        Geometry::RevolveAround { source, axis, .. } => vec![source, axis],
        Geometry::PlaceAtPoint { source, point } => vec![source, point],
        Geometry::SweepSketch { source, path, .. } => vec![source, path],
        Geometry::LoftSketches { sources, .. } => sources.iter().collect(),
        Geometry::Pipe { path, .. } => path_names(path),
        Geometry::PathPattern { source, path, .. } => {
            std::iter::once(source).chain(path_names(path)).collect()
        }
        Geometry::Mirror { source, .. }
        | Geometry::CircularPattern { source, .. }
        | Geometry::LinearPattern { source, .. }
        | Geometry::Transform { source, .. }
        | Geometry::Scale { source, .. }
        | Geometry::PlaneOnFace { source, .. }
        | Geometry::RevolveSketch { source, .. }
        | Geometry::ExtrudeSketch { source, .. }
        | Geometry::Shell { source, .. }
        | Geometry::Offset { source, .. }
        | Geometry::Thread { source, .. }
        | Geometry::Hole { source, .. }
        | Geometry::Split { source, .. }
        | Geometry::Thicken { source, .. }
        | Geometry::Draft { source, .. }
        | Geometry::Fillet { source, .. }
        | Geometry::Chamfer { source, .. } => vec![source],
        Geometry::Union { left, right }
        | Geometry::Difference { left, right }
        | Geometry::Intersection { left, right } => vec![left, right],
        _ => vec![],
    };
    names.into_iter().map(String::as_str).collect()
}

/// The input solid of a feature that changes one solid. Creation features and references have none.
pub(crate) fn primary_input<N>(geometry: &Geometry<N>) -> Option<&str> {
    match geometry {
        Geometry::Rib { body, .. }
        | Geometry::Web { body, .. }
        | Geometry::Boss { body, .. }
        | Geometry::SnapHook { body, .. }
        | Geometry::Lip { body, .. }
        | Geometry::Groove { body, .. } => Some(body),
        Geometry::Fillet { source, .. }
        | Geometry::Chamfer { source, .. }
        | Geometry::Shell { source, .. }
        | Geometry::Draft { source, .. }
        | Geometry::Offset { source, .. }
        | Geometry::Hole { source, .. }
        | Geometry::Thread { source, .. }
        | Geometry::Transform { source, .. }
        | Geometry::Scale { source, .. }
        | Geometry::Split { source, .. }
        | Geometry::LinearPattern { source, .. }
        | Geometry::CircularPattern { source, .. }
        | Geometry::PathPattern { source, .. }
        | Geometry::Mirror { source, .. } => Some(source),
        Geometry::Union { left, .. }
        | Geometry::Difference { left, .. }
        | Geometry::Intersection { left, .. } => Some(left),
        _ => None,
    }
}

/// Whether a feature gives a solid for later features. A suppressed feature gives its primary input.
pub(crate) fn resolves<N>(features: &[Feature<N>], id: &str) -> bool {
    let mut current = id;
    // Each step goes to an earlier feature. The bound stops a broken file with a cycle.
    for _ in 0..=features.len() {
        let Some(feature) = features.iter().find(|feature| feature.id == current) else {
            return false;
        };
        if !feature.suppressed {
            return true;
        }
        match primary_input(&feature.geometry) {
            Some(next) => current = next,
            None => return false,
        }
    }
    false
}

/// The feature id of a path, when the path names one.
fn path_names<N>(path: &PathInput<N>) -> Vec<&String> {
    match path {
        PathInput::Feature(name) => vec![name],
        PathInput::Curve(_) => vec![],
    }
}

fn load_persisted(store: &dyn FeatureCache, key: &str, kernel: &dyn Kernel) -> Option<Shape> {
    let bytes = store.get(key)?;
    let entry = feature_cache::decode(&bytes)?;
    let shape = kernel.decode_shape(entry.shape).ok()?;
    if kernel.face_count(&shape).ok()? != entry.faces {
        return None;
    }
    Some(match entry.origins {
        Some(origins) => shape.with_face_origins(origins),
        None => shape,
    })
}

fn store_built(store: &dyn FeatureCache, key: &str, kernel: &dyn Kernel, shape: &Shape) {
    if !store.writable() {
        return;
    }
    let (Ok(bytes), Ok(faces)) = (kernel.encode_shape(shape), kernel.face_count(shape)) else {
        return;
    };
    let blob = feature_cache::encode(faces, shape.face_origins(), &bytes);
    // A failed write only means that the next open builds this feature again.
    let _ = store.put(key, &blob);
}

pub(crate) fn build(
    features: &[Feature],
    output: &str,
    kernel: &dyn Kernel,
    cache: &mut Cache,
    paths: &crate::assets::Paths,
    persist: Option<&dyn FeatureCache>,
) -> Result<(Shape, Option<String>)> {
    if features.is_empty() || features.len() > 2000 {
        return Err("A history needs between 1 and 2000 features".into());
    }
    let version = kernel.cache_version();
    let persist = persist.filter(|_| version.is_some());
    let version = version.unwrap_or("");
    let suppressed: BTreeSet<&str> = features
        .iter()
        .filter(|feature| feature.suppressed)
        .map(|feature| feature.id.as_str())
        .collect();
    let mut keys = BTreeMap::<String, String>::new();
    let mut shapes = BTreeMap::<String, Built>::new();
    let mut ids = BTreeSet::new();
    let mut next = BTreeMap::new();
    for feature in features {
        if !valid_id(&feature.id) || !ids.insert(feature.id.to_ascii_lowercase()) {
            return Err(format!("Invalid or duplicate feature id: {}", feature.id));
        }
        // A suppressed feature is not built. With a primary input, it becomes an alias of that input.
        // The alias is not in the cache, so a later unsuppress builds the feature again.
        if feature.suppressed {
            if let Some(name) = primary_input(&feature.geometry)
                && let Some(value) = shapes.get(name).cloned()
            {
                if let Some(key) = keys.get(name).cloned() {
                    keys.insert(feature.id.clone(), key);
                }
                shapes.insert(feature.id.clone(), value);
            }
            continue;
        }
        let previous = |name: &str| {
            if suppressed.contains(name) && !shapes.contains_key(name) {
                return Err(format!(
                    "Feature {name} is suppressed. Unsuppress it or change the features that use it."
                ));
            }
            shapes
                .get(name)
                .ok_or_else(|| format!("Unknown previous feature: {name}"))
        };
        let source = |name: &str| match previous(name)? {
            Built::Shape(shape) => Ok(shape),
            _ => Err(format!("Feature {name} is a reference, not a shape")),
        };
        let dependencies = if let Geometry::SketchOnPlane { plane, .. } = &feature.geometry {
            vec![previous(plane)?.clone()]
        } else if let Geometry::RevolveAround {
            source: name,
            axis: reference,
            ..
        }
        | Geometry::PlaceAtPoint {
            source: name,
            point: reference,
        }
        | Geometry::Rib {
            body: name,
            plane: reference,
            ..
        }
        | Geometry::Web {
            body: name,
            plane: reference,
            ..
        }
        | Geometry::Boss {
            body: name,
            plane: reference,
            ..
        }
        | Geometry::SnapHook {
            body: name,
            plane: reference,
            ..
        } = &feature.geometry
        {
            vec![
                Built::Shape(source(name)?.clone()),
                previous(reference)?.clone(),
            ]
        } else if let Some(inputs) = construction_inputs(&feature.geometry, &shapes)
            .map_err(|error| named(feature, error))?
        {
            inputs
        } else {
            match &feature.geometry {
                Geometry::SweepSketch {
                    source: name, path, ..
                } => {
                    vec![source(name)?.clone(), source(path)?.clone()]
                }
                Geometry::LoftSketches { sources, .. } => sources
                    .iter()
                    .map(|name| source(name).cloned())
                    .collect::<Result<Vec<_>>>()?,
                Geometry::Pipe {
                    path: PathInput::Feature(name),
                    ..
                } => vec![source(name)?.clone()],
                Geometry::PathPattern {
                    source: name, path, ..
                } => {
                    let mut inputs = vec![source(name)?.clone()];
                    if let PathInput::Feature(id) = path {
                        inputs.push(source(id)?.clone());
                    }
                    inputs
                }
                Geometry::Mirror { source: name, .. }
                | Geometry::CircularPattern { source: name, .. }
                | Geometry::LinearPattern { source: name, .. }
                | Geometry::Transform { source: name, .. }
                | Geometry::Scale { source: name, .. }
                | Geometry::PlaneOnFace { source: name, .. }
                | Geometry::RevolveSketch { source: name, .. }
                | Geometry::ExtrudeSketch { source: name, .. }
                | Geometry::Shell { source: name, .. }
                | Geometry::Offset { source: name, .. }
                | Geometry::Thread { source: name, .. }
                | Geometry::Hole { source: name, .. }
                | Geometry::Split { source: name, .. }
                | Geometry::Thicken { source: name, .. }
                | Geometry::Draft { source: name, .. }
                | Geometry::Fillet { source: name, .. }
                | Geometry::Chamfer { source: name, .. } => vec![source(name)?.clone()],
                Geometry::Lip { body, .. } | Geometry::Groove { body, .. } => {
                    vec![source(body)?.clone()]
                }
                Geometry::Union { left, right }
                | Geometry::Difference { left, right }
                | Geometry::Intersection { left, right } => {
                    vec![source(left)?.clone(), source(right)?.clone()]
                }
                _ => vec![],
            }
            .into_iter()
            .map(Built::Shape)
            .collect()
        };
        let key = match persist {
            Some(_) => {
                let inputs = references(&feature.geometry)
                    .into_iter()
                    .map(|name| {
                        keys.get(name)
                            .cloned()
                            .ok_or_else(|| format!("Unknown previous feature: {name}"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let key = feature_cache::key(version, &feature.id, &feature.geometry, &inputs);
                keys.insert(feature.id.clone(), key.clone());
                Some(key)
            }
            None => None,
        };
        if let Some(entry) = cache.0.get(&feature.id)
            && entry.geometry == feature.geometry
            && entry.dependencies.len() == dependencies.len()
            && entry
                .dependencies
                .iter()
                .zip(&dependencies)
                .all(|(a, b)| a.same_input(b))
        {
            shapes.insert(feature.id.clone(), entry.value.clone());
            next.insert(feature.id.clone(), entry.clone());
            continue;
        }
        let reference_value = match &feature.geometry {
            Geometry::PlaneOnFace {
                source: name,
                face,
                offset,
            } => crate::select::plane(kernel, source(name)?, face, *offset)
                .map(|plane| Some(Built::Plane(plane))),
            Geometry::PlaneAtAngle { .. }
            | Geometry::Midplane { .. }
            | Geometry::PlaneThroughPoints { .. }
            | Geometry::AxisFromCylinder { .. }
            | Geometry::AxisFromPlanes { .. }
            | Geometry::PointAtCentre { .. } => {
                construction_value(kernel, &feature.geometry, &dependencies).map(Some)
            }
            geometry => reference(geometry),
        };
        if let Some(value) = reference_value.map_err(|error| named(feature, error))? {
            next.insert(
                feature.id.clone(),
                Entry {
                    geometry: feature.geometry.clone(),
                    dependencies,
                    value: value.clone(),
                },
            );
            shapes.insert(feature.id.clone(), value);
            continue;
        }
        if let Some(store) = persist
            && let Some(key) = &key
            && let Some(shape) = load_persisted(store, key, kernel)
        {
            next.insert(
                feature.id.clone(),
                Entry {
                    geometry: feature.geometry.clone(),
                    dependencies,
                    value: Built::Shape(shape.clone()),
                },
            );
            shapes.insert(feature.id.clone(), Built::Shape(shape));
            continue;
        }
        let build = || -> Result<Shape> {
            match &feature.geometry {
                geometry @ (Geometry::Rib { body, plane, .. }
                | Geometry::Web { body, plane, .. }
                | Geometry::Boss { body, plane, .. }
                | Geometry::SnapHook { body, plane, .. }) => {
                    let Built::Plane(work) = previous(plane)? else {
                        return Err(format!("Feature {plane} is not a reference plane"));
                    };
                    let body = source(body)?;
                    match geometry {
                        Geometry::Boss { .. } => crate::plastic::boss(kernel, body, work, geometry),
                        Geometry::SnapHook { .. } => {
                            crate::plastic::snap_hook(kernel, body, work, geometry)
                        }
                        _ => crate::plastic_walls::walls(kernel, body, work, geometry),
                    }
                }
                geometry @ (Geometry::Lip { body, .. } | Geometry::Groove { body, .. }) => {
                    crate::plastic_walls::rim(kernel, source(body)?, geometry)
                }
                Geometry::RevolveAround {
                    source: name,
                    axis,
                    angle,
                } => {
                    let Built::Axis { origin, direction } = previous(axis)? else {
                        return Err(format!("Feature {axis} is not a reference axis"));
                    };
                    kernel.revolve_shape(source(name)?, *origin, *direction, *angle)
                }
                Geometry::PlaceAtPoint {
                    source: name,
                    point,
                } => {
                    let Built::Point(position) = previous(point)? else {
                        return Err(format!("Feature {point} is not a reference point"));
                    };
                    kernel.transform_shape(
                        source(name)?,
                        &faber_model::Placement {
                            translation: *position,
                            rotation: [0.; 3],
                        },
                    )
                }
                Geometry::SketchOnPlane {
                    plane,
                    profile,
                    placement,
                } => {
                    let Built::Plane(reference) = previous(plane)? else {
                        return Err(format!("Feature {plane} is not a reference plane"));
                    };
                    let sketch = kernel.sketch(profile, placement)?;
                    kernel.transform_shape(&sketch, reference)
                }
                Geometry::Imported { asset, .. } => {
                    kernel.read_shape(paths.get(asset).ok_or("Asset is missing")?)
                }
                Geometry::Mirror {
                    source: name,
                    origin,
                    normal,
                    keep_source,
                } => {
                    let original = source(name)?;
                    let mirrored = kernel.mirror_shape(original, *origin, *normal)?;
                    if *keep_source {
                        kernel.combine(original, &mirrored, Combine::Union)
                    } else {
                        Ok(mirrored)
                    }
                }
                Geometry::CircularPattern {
                    source: name,
                    count,
                    origin,
                    axis,
                    angle,
                } => {
                    crate::pattern::circular(kernel, source(name)?, *count, *origin, *axis, *angle)
                }
                Geometry::LinearPattern {
                    source: name,
                    count,
                    step,
                } => crate::pattern::linear(kernel, source(name)?, *count, *step),
                Geometry::PathPattern {
                    source: name,
                    path,
                    count,
                    spacing,
                    orient,
                } => {
                    let path = match path {
                        PathInput::Feature(id) => source(id)?.clone(),
                        PathInput::Curve(curve) => kernel.curve(curve)?,
                    };
                    crate::pattern::along_path(
                        kernel,
                        source(name)?,
                        &path,
                        *count,
                        *spacing,
                        *orient,
                    )
                }
                Geometry::Pipe {
                    path,
                    diameter,
                    wall,
                } => {
                    let path = match path {
                        PathInput::Feature(id) => source(id)?.clone(),
                        PathInput::Curve(curve) => kernel.curve(curve)?,
                    };
                    crate::pipe::build(kernel, &path, *diameter, *wall)
                }
                Geometry::Scale {
                    source: name,
                    factor,
                    about,
                } => crate::scale::build(kernel, source(name)?, factor, *about),
                Geometry::Curve { path } => kernel.curve(path),
                Geometry::Helix {
                    radius,
                    pitch,
                    turns,
                    placement,
                } => {
                    let shape = kernel.helix(*radius, *pitch, *turns)?;
                    kernel.transform_shape(&shape, placement)
                }
                Geometry::SweepSketch {
                    source: name,
                    path,
                    frenet,
                } => kernel.sweep_shape(source(name)?, source(path)?, *frenet),
                Geometry::RevolveSketch {
                    source: name,
                    origin,
                    axis,
                    angle,
                } => kernel.revolve_shape(source(name)?, *origin, *axis, *angle),
                Geometry::LoftSketches { sources, ruled } => {
                    let sections = sources
                        .iter()
                        .map(|name| source(name).cloned())
                        .collect::<Result<Vec<_>>>()?;
                    kernel.loft_shapes(&sections, *ruled)
                }
                Geometry::Sketch { profile, placement } => kernel.sketch(profile, placement),
                Geometry::ExtrudeSketch {
                    source: name,
                    vector,
                } => kernel.extrude_shape(source(name)?, *vector),
                Geometry::Draft {
                    source: name,
                    faces,
                    direction,
                    origin,
                    normal,
                    angle,
                } => {
                    let shape = source(name)?;
                    let faces = crate::select::faces(kernel, shape, faces)?;
                    kernel.draft(shape, &faces, *direction, *origin, *normal, *angle)
                }
                Geometry::Thicken {
                    source: name,
                    thickness,
                    face,
                } => {
                    let shape = source(name)?;
                    if let Some(selector) = face {
                        let faces = crate::select::faces(kernel, shape, selector)?;
                        if faces.len() != 1 {
                            return Err("Thicken needs exactly one selected face".into());
                        }
                        kernel.thicken(&kernel.face_shape(shape, faces[0])?, *thickness)
                    } else {
                        kernel.thicken(shape, *thickness)
                    }
                }
                Geometry::Split {
                    source: name,
                    origin,
                    normal,
                    keep,
                } => kernel.split(source(name)?, *origin, *normal, *keep),
                Geometry::Thread {
                    source: name,
                    face,
                    size,
                    length,
                    offset,
                    left_handed,
                } => crate::thread::build(
                    kernel,
                    source(name)?,
                    face,
                    size,
                    *length,
                    *offset,
                    *left_handed,
                ),
                Geometry::Hole {
                    source: name,
                    diameter,
                    depth,
                    placement,
                    head,
                } => crate::hole::build(
                    kernel,
                    source(name)?,
                    *diameter,
                    *depth,
                    placement,
                    head.as_ref(),
                ),
                Geometry::Offset {
                    source: name,
                    distance,
                } => kernel.offset(source(name)?, *distance),
                Geometry::Shell {
                    source: name,
                    faces,
                    thickness,
                    thickness_overrides,
                } => {
                    let shape = source(name)?;
                    if not_positive(*thickness) {
                        return Err(SHELL_SIGN_RULE.into());
                    }
                    let removed = crate::select::faces(kernel, shape, faces)?;
                    let mut claimed: BTreeSet<u32> = removed.iter().copied().collect();
                    let mut offsets = Vec::new();
                    for rule in thickness_overrides {
                        if not_positive(rule.thickness) {
                            return Err(SHELL_SIGN_RULE.into());
                        }
                        let matched = crate::select::faces(kernel, shape, &rule.faces)?;
                        for face in matched {
                            if !claimed.insert(face) {
                                return Err(SHELL_CONFLICT.into());
                            }
                            offsets.push((face, rule.thickness));
                        }
                    }
                    dress_with_hint(*thickness, |value| {
                        kernel.shell(shape, &removed, value, &offsets)
                    })
                    .map_err(|error| {
                        format!(
                            "Feature {} (shell) failed: {error}. {SHELL_HINT}",
                            feature.id
                        )
                    })
                }
                Geometry::Fillet {
                    source: name,
                    edges,
                    radius,
                    radius_end,
                } => {
                    crate::dress_variant::check_fillet(*radius, *radius_end)?;
                    let shape = source(name)?;
                    let edges = crate::select::edges(kernel, shape, edges)?;
                    match radius_end {
                        // The end radius scales with the start radius during the hint search.
                        Some(end) => dress_with_hint(*radius, |value| {
                            kernel.fillet_variable(shape, &edges, value, end * value / radius)
                        }),
                        None => {
                            dress_with_hint(*radius, |value| kernel.fillet(shape, &edges, value))
                        }
                    }
                }
                Geometry::Chamfer {
                    source: name,
                    edges,
                    distance,
                    distance2,
                    angle_deg,
                    face,
                } => {
                    crate::dress_variant::check_chamfer(*distance, *distance2, *angle_deg)?;
                    let shape = source(name)?;
                    let edges = crate::select::edges(kernel, shape, edges)?;
                    let face = match face {
                        Some(selector) => {
                            Some(crate::dress_variant::chamfer_face(kernel, shape, selector)?)
                        }
                        None => None,
                    };
                    match (distance2, angle_deg) {
                        // The second distance scales with the first during the hint search.
                        (Some(second), _) => dress_with_hint(*distance, |value| {
                            kernel.chamfer_two(
                                shape,
                                &edges,
                                value,
                                second * value / distance,
                                face,
                            )
                        }),
                        (None, Some(angle)) => dress_with_hint(*distance, |value| {
                            kernel.chamfer_angle(shape, &edges, value, *angle, face)
                        }),
                        (None, None) => {
                            dress_with_hint(*distance, |value| kernel.chamfer(shape, &edges, value))
                        }
                    }
                }
                Geometry::History { .. } => {
                    Err("Nested feature histories are not supported".into())
                }
                Geometry::Transform {
                    source: name,
                    placement,
                } => kernel.transform_shape(source(name)?, placement),
                Geometry::Union { left, right } => {
                    kernel.combine(source(left)?, source(right)?, Combine::Union)
                }
                Geometry::Difference { left, right } => {
                    kernel.combine(source(left)?, source(right)?, Combine::Difference)
                }
                Geometry::Intersection { left, right } => {
                    kernel.combine(source(left)?, source(right)?, Combine::Intersection)
                }
                geometry => kernel
                    .exact_shape(geometry)?
                    .ok_or_else(|| "Feature needs an exact solid".into()),
            }
        };
        let shape = build().map_err(|error| named(feature, error))?;
        let properties = kernel
            .summary_properties(&shape)
            .map_err(|error| named(feature, error))?;
        let (sketch, curve) = match &feature.geometry {
            Geometry::Sketch { .. } | Geometry::SketchOnPlane { .. } => (true, false),
            Geometry::Curve { .. } | Geometry::Helix { .. } => (false, true),
            Geometry::Transform { source: name, .. }
            | Geometry::PlaceAtPoint { source: name, .. } => {
                let original = kernel
                    .summary_properties(source(name)?)
                    .map_err(|error| named(feature, error))?;
                if properties.solids != original.solids
                    || properties.faces != original.faces
                    || properties.edges != original.edges
                {
                    return Err(named(
                        feature,
                        "changed the source topology during placement".into(),
                    ));
                }
                (
                    original.solids == 0 && original.faces > 0,
                    original.solids == 0 && original.faces == 0 && original.edges > 0,
                )
            }
            _ => (false, false),
        };
        if !properties.valid
            || (sketch && (properties.faces == 0 || properties.solids != 0))
            || (curve && (properties.edges == 0 || properties.faces != 0 || properties.solids != 0))
            || (!sketch
                && !curve
                && (properties.solids == 0
                    || !properties.volume.is_finite()
                    || properties.volume <= 0.))
        {
            return Err(named(feature, "did not produce a valid solid".into()));
        }
        let inputs: Vec<_> = dependencies
            .iter()
            .filter_map(|value| match value {
                Built::Shape(shape) => Some(shape.clone()),
                _ => None,
            })
            .collect();
        let shape = crate::provenance::track(kernel, shape, &inputs, &feature.id)
            .map_err(|error| named(feature, error))?;
        if let (Some(store), Some(key)) = (persist, &key) {
            store_built(store, key, kernel, &shape);
        }
        next.insert(
            feature.id.clone(),
            Entry {
                geometry: feature.geometry.clone(),
                dependencies,
                value: Built::Shape(shape.clone()),
            },
        );
        shapes.insert(feature.id.clone(), Built::Shape(shape));
    }
    let result = shapes.remove(output).ok_or_else(|| {
        if suppressed.contains(output) {
            format!("The output feature has no solid to show: {output}")
        } else {
            format!("Unknown output feature: {output}")
        }
    })?;
    match result {
        Built::Shape(shape) => {
            cache.0 = next;
            // The key is set only when the persistent cache is active.
            Ok((shape, keys.get(output).cloned()))
        }
        _ => Err("A reference cannot be the part output".into()),
    }
}
