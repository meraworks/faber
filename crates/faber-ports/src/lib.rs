//! Ports for geometry, storage and output.

use faber_model::{Geometry, ProjectData};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::Path;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, String>;

pub trait AssetStore: Send + Sync {
    fn put(&self, source: &Path) -> Result<String>;
    fn resolve(&self, key: &str) -> Result<std::path::PathBuf>;
    /// Read a polygon surface. None means the asset needs an exact shape reader.
    fn read_mesh(&self, _path: &Path) -> Result<Option<Mesh>> {
        Ok(None)
    }
}

/// Design checks. Declared contacts give no violation.
pub trait DesignChecks: Send + Sync {
    fn errors(
        &self,
        contacts: &std::collections::BTreeMap<String, faber_model::Contact>,
        bodies: &[Body],
    ) -> Result<std::collections::BTreeMap<String, String>>;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// Surface normals per vertex. Empty when the source has no surface normals.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normals: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub group: String,
    pub colour: [u8; 3],
    pub mesh: Mesh,
    pub volume: f64,
    pub bounds: [[f64; 3]; 2],
    #[serde(skip)]
    pub shape: Option<Shape>,
}

/// A point on a path and its unit tangent.
#[derive(Clone, Debug, PartialEq)]
pub struct PathSample {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
}

/// The length of a path and the samples at the requested distances.
#[derive(Clone, Debug, PartialEq)]
pub struct PathSamples {
    pub length: f64,
    pub samples: Vec<PathSample>,
}

pub trait Kernel: Send + Sync {
    fn face_sources(&self, _shape: &Shape, _inputs: &[Shape]) -> Result<Option<Vec<FaceSource>>> {
        Ok(None)
    }
    fn edge_faces(&self, _shape: &Shape) -> Result<Vec<EdgeFace>> {
        Err("Edge and face adjacency is not available".into())
    }
    fn split(
        &self,
        _shape: &Shape,
        _origin: [f64; 3],
        _normal: [f64; 3],
        _keep: faber_model::SplitKeep,
    ) -> Result<Shape> {
        Err("Split is not available".into())
    }
    fn face_shape(&self, _shape: &Shape, _index: u32) -> Result<Shape> {
        Err("Face extraction is not available".into())
    }
    fn thicken(&self, _shape: &Shape, _thickness: f64) -> Result<Shape> {
        Err("Thicken is not available".into())
    }
    fn draft(
        &self,
        _shape: &Shape,
        _faces: &[u32],
        _direction: [f64; 3],
        _origin: [f64; 3],
        _normal: [f64; 3],
        _angle: f64,
    ) -> Result<Shape> {
        Err("Draft is not available".into())
    }
    fn offset(&self, _shape: &Shape, _distance: f64) -> Result<Shape> {
        Err("Offset is not available".into())
    }
    fn distance(&self, _a: &Shape, _b: &Shape) -> Result<Distance> {
        Err("Distance measurement is not available".into())
    }
    fn mass_properties(&self, _shape: &Shape) -> Result<MassProperties> {
        Err("Mass properties are not available".into())
    }
    fn mirror_shape(&self, _shape: &Shape, _origin: [f64; 3], _normal: [f64; 3]) -> Result<Shape> {
        Err("Mirror is not available".into())
    }
    fn helix(&self, _radius: f64, _pitch: f64, _turns: f64) -> Result<Shape> {
        Err("Helix is not available".into())
    }
    fn curve(&self, _path: &faber_model::CurvePath) -> Result<Shape> {
        Err("Curve is not available".into())
    }
    fn sweep_shape(&self, _section: &Shape, _path: &Shape, _frenet: bool) -> Result<Shape> {
        Err("Sketch sweep is not available".into())
    }
    /// Samples a curve wire at arc-length distances from its start.
    fn path_samples(&self, _path: &Shape, _distances: &[f64]) -> Result<PathSamples> {
        Err("Path sampling is not available".into())
    }
    /// Scales a shape about `about`. Equal factors keep the shape uniform.
    fn scale_shape(&self, _shape: &Shape, _about: [f64; 3], _factor: [f64; 3]) -> Result<Shape> {
        Err("Scale is not available".into())
    }
    fn revolve_shape(
        &self,
        _shape: &Shape,
        _origin: [f64; 3],
        _axis: [f64; 3],
        _angle: f64,
    ) -> Result<Shape> {
        Err("Sketch revolve is not available".into())
    }
    fn loft_shapes(&self, _sections: &[Shape], _ruled: bool) -> Result<Shape> {
        Err("Sketch loft is not available".into())
    }
    fn sketch(
        &self,
        _profile: &faber_model::SketchProfile,
        _placement: &faber_model::Placement,
    ) -> Result<Shape> {
        Err("Sketch is not available".into())
    }
    fn extrude_shape(&self, _shape: &Shape, _vector: [f64; 3]) -> Result<Shape> {
        Err("Sketch extrusion is not available".into())
    }
    fn faces(&self, _shape: &Shape) -> Result<Vec<FaceInfo>> {
        Err("Face queries are not available".into())
    }
    /// Remove `faces`, then hollow with `thickness`. `offsets` gives a thickness per face index.
    fn shell(
        &self,
        _shape: &Shape,
        _faces: &[u32],
        _thickness: f64,
        _offsets: &[(u32, f64)],
    ) -> Result<Shape> {
        Err("Shell is not available".into())
    }
    fn edges(&self, _shape: &Shape) -> Result<Vec<EdgeInfo>> {
        Err("Edge queries are not available".into())
    }
    fn fillet(&self, _shape: &Shape, _edges: &[u32], _radius: f64) -> Result<Shape> {
        Err("Fillet is not available".into())
    }
    fn chamfer(&self, _shape: &Shape, _edges: &[u32], _distance: f64) -> Result<Shape> {
        Err("Chamfer is not available".into())
    }
    /// Round `edges`. The radius changes linearly from `start` to `end`.
    fn fillet_variable(
        &self,
        _shape: &Shape,
        _edges: &[u32],
        _start: f64,
        _end: f64,
    ) -> Result<Shape> {
        Err("Variable fillet is not available".into())
    }
    /// Chamfer `edges` with two distances. `first` is measured on `face`.
    /// Without `face`, the kernel uses the first face of each edge.
    fn chamfer_two(
        &self,
        _shape: &Shape,
        _edges: &[u32],
        _first: f64,
        _second: f64,
        _face: Option<u32>,
    ) -> Result<Shape> {
        Err("Chamfer is not available".into())
    }
    /// Chamfer `edges` with a distance and an angle in degrees, measured from `face`.
    /// Without `face`, the kernel uses the first face of each edge.
    fn chamfer_angle(
        &self,
        _shape: &Shape,
        _edges: &[u32],
        _distance: f64,
        _angle_deg: f64,
        _face: Option<u32>,
    ) -> Result<Shape> {
        Err("Chamfer is not available".into())
    }
    fn sweep(
        &self,
        _profile: &faber_model::Profile,
        _placement: &faber_model::Placement,
        _path: &[[f64; 3]],
    ) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn loft(
        &self,
        _sections: &[(faber_model::Profile, faber_model::Placement)],
        _ruled: bool,
    ) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn build(&self, geometry: &Geometry) -> Result<Mesh>;
    fn exact_shape(&self, _geometry: &Geometry) -> Result<Option<Shape>> {
        Ok(None)
    }

    fn box_shape(&self, _size: [f64; 3]) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn cylinder_shape(&self, _radius: f64, _height: f64) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn sphere_shape(&self, _radius: f64) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn cone_shape(&self, _base_radius: f64, _top_radius: f64, _height: f64) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn torus_shape(&self, _major_radius: f64, _minor_radius: f64) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn revolve(
        &self,
        _profile: &faber_model::Profile,
        _origin: [f64; 3],
        _axis: [f64; 3],
        _angle: f64,
    ) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn transform_shape(
        &self,
        _shape: &Shape,
        _placement: &faber_model::Placement,
    ) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    fn combine(&self, _left: &Shape, _right: &Shape, _operation: Combine) -> Result<Shape> {
        Err("Exact solids are not available".into())
    }
    /// Common part of two shapes. Returns `None` when the shapes share no solid.
    fn intersect_solids(&self, _left: &Shape, _right: &Shape) -> Result<Option<Shape>> {
        Err("Exact solids are not available".into())
    }
    fn properties(&self, _shape: &Shape) -> Result<Properties> {
        Err("Exact solids are not available".into())
    }
    fn tessellate(&self, _shape: &Shape, _tolerance: f64) -> Result<Mesh> {
        Err("Exact solids are not available".into())
    }
    fn write_shape(&self, _shape: &Shape, _path: &Path) -> Result<()> {
        Err("Exact shape output is not available".into())
    }
    fn read_shape(&self, _path: &Path) -> Result<Shape> {
        Err("Exact shape input is not available".into())
    }
    /// Version of the encoding. None turns the persistent feature cache off.
    fn cache_version(&self) -> Option<&'static str> {
        None
    }
    /// Opaque bytes that hold one exact shape. The bytes come from `decode_shape`.
    fn encode_shape(&self, _shape: &Shape) -> Result<Vec<u8>> {
        Err("Shape encoding is not available".into())
    }
    fn decode_shape(&self, _bytes: &[u8]) -> Result<Shape> {
        Err("Shape decoding is not available".into())
    }
    fn face_count(&self, shape: &Shape) -> Result<u32> {
        Ok(self.faces(shape)?.len() as u32)
    }
    /// Like `properties`, with zero bounds. Use it when the bounds are not needed.
    fn summary_properties(&self, shape: &Shape) -> Result<Properties> {
        self.properties(shape)
    }
}

/// Stores the opaque bytes of built features. A missing or invalid entry returns None.
pub trait FeatureCache: Send + Sync {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()>;
}

pub type FaceOrigins = std::collections::BTreeMap<u32, std::collections::BTreeSet<String>>;

#[derive(Clone)]
pub struct Shape(Arc<dyn Any + Send + Sync>, Option<Arc<FaceOrigins>>);

impl std::fmt::Debug for Shape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Shape")
    }
}

impl Shape {
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value), None)
    }
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
    pub fn with_face_origins(mut self, origins: FaceOrigins) -> Self {
        self.1 = Some(Arc::new(origins));
        self
    }
    pub fn face_origins(&self) -> Option<&FaceOrigins> {
        self.1.as_deref()
    }
}

pub struct FaceSource {
    pub face: u32,
    pub input: usize,
    pub source_face: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum Combine {
    Union,
    Difference,
    Intersection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Properties {
    pub volume: f64,
    pub centre: [f64; 3],
    pub bounds: [[f64; 3]; 2],
    pub faces: u32,
    pub edges: u32,
    pub solids: u32,
    pub valid: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub index: u32,
    pub length: f64,
    pub centre: [f64; 3],
    pub direction: Option<[f64; 3]>,
    pub radius: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CylinderFace {
    pub start: [f64; 3],
    pub axis: [f64; 3],
    pub radius: f64,
    pub length: f64,
    pub internal: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cylinder: Option<CylinderFace>,
    pub index: u32,
    /// Source feature names; absent when face history is not available.
    pub created_by: Option<Vec<String>>,
    pub area: f64,
    pub centre: [f64; 3],
    /// Outward unit normal for a planar face; absent for curved faces.
    pub normal: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeFace {
    pub edge: u32,
    pub face: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Distance {
    pub distance_mm: f64,
    pub point_a: [f64; 3],
    pub point_b: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MassProperties {
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub centre_of_mass_mm: [f64; 3],
    /// Inertia about the centre of mass for unit density, in mm5.
    pub inertia_unit_mm5: [[f64; 3]; 3],
}

pub trait ProjectStore: Send + Sync {
    fn checkpoint(&self, _name: &str, _expected: &ProjectData) -> Result<()> {
        Err("Checkpoints are not available".into())
    }
    fn read_checkpoint(&self, _name: &str) -> Result<ProjectData> {
        Err("Checkpoints are not available".into())
    }
    fn load(&self) -> Result<ProjectData>;
    fn save(&self, expected_revision: Option<u64>, data: &ProjectData) -> Result<()>;
    fn save_checked(&self, expected: &ProjectData, data: &ProjectData) -> Result<()> {
        if self.load()? != *expected {
            return Err("Project changed before save".into());
        }
        self.save(Some(expected.revision), data)
    }
    fn history_target(&self, _action: HistoryMove) -> Result<ProjectData> {
        Err("History is not available".into())
    }
    fn save_history(
        &self,
        _expected: &ProjectData,
        _data: &ProjectData,
        _action: HistoryMove,
    ) -> Result<()> {
        Err("History is not available".into())
    }
}

#[derive(Clone, Copy)]
pub enum HistoryMove {
    Undo,
    Redo,
}

pub trait Exchange: Send + Sync {
    fn write(&self, mesh: &Mesh, path: &Path) -> Result<()>;
}

/// True when an id is a valid part or feature name. The id is also a file name.
/// Only ASCII letters, digits, `_` and `-` pass. Windows device names do not pass.
pub fn valid_id(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    !reserved
        && !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
