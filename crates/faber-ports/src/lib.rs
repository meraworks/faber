//! Ports for geometry, storage and output.

use faber_model::{Geometry, ProjectData};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::Path;
use std::sync::Arc;

pub mod bom;
pub mod drawing;
pub mod source;
pub mod vector;

pub type Result<T> = std::result::Result<T, String>;

pub trait AssetStore: Send + Sync {
    /// Return the content key without publishing the asset.
    fn key(&self, _source: &Path) -> Result<String> {
        Err("Staged assets are not available.".into())
    }
    fn put(&self, source: &Path) -> Result<String>;
    fn resolve(&self, key: &str) -> Result<std::path::PathBuf>;
    /// Read a polygon surface. None means the asset needs an exact shape reader.
    fn read_mesh(&self, _path: &Path) -> Result<Option<Mesh>> {
        Ok(None)
    }
    /// Return true when the asset contains only a triangle surface.
    fn is_mesh(&self, path: &Path) -> Result<bool> {
        Ok(self.read_mesh(path)?.is_some())
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

impl Mesh {
    /// Visit the three vertices of each triangle in order.
    pub fn triangle_vertices(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.triangles
            .iter()
            .flatten()
            .map(|&i| self.vertices[i as usize])
    }

    /// Join referenced vertices on the same tolerance grid. An unused vertex maps to u32::MAX.
    pub fn welded_vertices(&self, tolerance_mm: f64) -> Result<(Vec<[f64; 3]>, Vec<u32>)> {
        if !tolerance_mm.is_finite() || tolerance_mm <= 0. {
            return Err("The weld tolerance must be finite and positive.".into());
        }
        let mut used = vec![false; self.vertices.len()];
        for &i in self.triangles.iter().flatten() {
            *used
                .get_mut(i as usize)
                .ok_or("A triangle refers to a missing vertex.")? = true;
        }
        let mut remap = vec![u32::MAX; self.vertices.len()];
        let mut points = Vec::new();
        let mut grid = std::collections::HashMap::new();
        let mut origin = None;
        for (i, &p) in self.vertices.iter().enumerate() {
            if !p.into_iter().all(f64::is_finite) {
                return Err("Mesh coordinates must be finite.".into());
            }
            if !used[i] {
                continue;
            }
            let base = origin.get_or_insert(p);
            let mut key = [0i64; 3];
            for axis in 0..3 {
                let value = ((p[axis] - base[axis]) / tolerance_mm).round();
                if value.abs() >= i64::MAX as f64 {
                    return Err("Mesh coordinates exceed the welding range.".into());
                }
                key[axis] = value as i64;
            }
            if points.len() >= u32::MAX as usize {
                return Err("The mesh has too many vertices.".into());
            }
            remap[i] = *grid.entry(key).or_insert_with(|| {
                let id = points.len() as u32;
                points.push(p);
                id
            });
        }
        Ok((points, remap))
    }
}

/// One mesh in its local coordinates. The last three transform values give translation.
pub struct MeshItem<'a> {
    pub name: &'a str,
    pub colour: [u8; 3],
    pub mesh: &'a Mesh,
    /// Columns of the three world axes, then the world origin of the mesh.
    pub transform: [f64; 12],
}

pub trait MeshAssemblyExport {
    /// Return the number of written triangles.
    fn write_assembly(&self, items: &[MeshItem<'_>], path: &Path) -> Result<usize>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceMesh {
    pub mesh: Mesh,
    /// One face index from the topology query for each triangle.
    pub face_of_triangle: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Sphere,
    Cone,
    Torus,
    Freeform,
}

#[derive(Clone, Debug)]
pub struct CurvedMesh {
    pub faces: FaceMesh,
    /// Principal curvatures per vertex, in ascending order. Convex is positive.
    pub curvature: Vec<[f64; 2]>,
    /// Surface type at each topology face index. Indices start at one.
    pub kinds: std::collections::BTreeMap<u32, SurfaceKind>,
}

pub const FACE_MESH_UNSUPPORTED: &str = "Face mesh is not supported";
pub const EDGE_LINES_UNSUPPORTED: &str = "Edge lines are not supported";

/// Each range starts at the first point of an edge. Edge indices start at one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EdgeLines {
    pub points: Vec<[f64; 3]>,
    pub ranges: Vec<std::ops::Range<u32>>,
}

impl EdgeLines {
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn edge(&self, id: u32) -> Option<&[[f64; 3]]> {
        let range = self.ranges.get(id.checked_sub(1)? as usize)?;
        self.points.get(range.start as usize..range.end as usize)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub group: String,
    pub colour: [u8; 3],
    pub mesh: Mesh,
    /// Empty when the source has no face indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_of_triangle: Vec<u32>,
    #[serde(default, skip_serializing_if = "EdgeLines::is_empty")]
    pub edge_lines: EdgeLines,
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

pub const SOLID_TRIANGLE_LIMIT: usize = 200_000;

pub trait Kernel: Send + Sync {
    fn project_edges(
        &self,
        _shapes: &[Shape],
        _frame: &drawing::ViewFrame,
        _hidden: bool,
    ) -> Result<drawing::Projection2> {
        Err("Drawing projection is not available.".into())
    }
    fn section_face(&self, _shape: &Shape, _origin: [f64; 3], _normal: [f64; 3]) -> Result<Shape> {
        Err("Section faces are not available.".into())
    }
    /// Make a solid from closed, consistently wound triangles. Optionally merge coplanar faces.
    fn solid_from_mesh(&self, _mesh: &Mesh, _merge_planar: bool) -> Result<Shape> {
        Err("Mesh to solid conversion is not available.".into())
    }
    /// Grow a wall from a line to the next face. Offset ranges from -1 to 1.
    fn linear_form(
        &self,
        _body: &Shape,
        _profile: &Shape,
        _direction: [f64; 3],
        _thickness: f64,
        _offset: f64,
    ) -> Result<Shape> {
        Err("Wall growth is not available".into())
    }
    /// A planar face bounded by one offset loop. Loop zero is the outer loop.
    fn inset_face(&self, _face: &Shape, _loop_index: u32, _distance: f64) -> Result<Shape> {
        Err("Face loop offsets are not available".into())
    }
    /// Offset the full boundary of a planar face, including its holes.
    fn offset_sketch_face(&self, _face: &Shape, _distance: f64) -> Result<Shape> {
        Err("Sketch offsets are not available.".into())
    }
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
    fn remove_faces(&self, _shape: &Shape, _faces: &[u32]) -> Result<Shape> {
        Err("Face removal is not available.".into())
    }
    fn move_faces(&self, _shape: &Shape, _faces: &[u32], _distance: f64) -> Result<Shape> {
        Err("Face movement is not available.".into())
    }
    fn split_face(&self, _shape: &Shape, _face: u32, _tool: &Shape) -> Result<Shape> {
        Err("Face splitting is not available.".into())
    }
    fn face_shape(&self, _shape: &Shape, _index: u32) -> Result<Shape> {
        Err("Face extraction is not available".into())
    }
    /// Wrap sketch X as arc length and sketch Y along the cylinder axis.
    fn wrap_on_cylinder(
        &self,
        _flat: &Shape,
        _cylinder: &CylinderFace,
        _at: [f64; 2],
    ) -> Result<Shape> {
        Err("Cylindrical wrapping is not supported.".into())
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
    fn tessellate_faces(&self, _shape: &Shape, _tolerance: f64) -> Result<FaceMesh> {
        Err(FACE_MESH_UNSUPPORTED.into())
    }
    fn tessellate_curvature(&self, _shape: &Shape, _tolerance: f64) -> Result<CurvedMesh> {
        Err("Surface curvature is not supported.".into())
    }
    fn tessellate_edges(&self, _shape: &Shape, _tolerance: f64) -> Result<EdgeLines> {
        Err(EDGE_LINES_UNSUPPORTED.into())
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
    fn writable(&self) -> bool {
        true
    }

    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn get_bounded(&self, key: &str, max_bytes: usize) -> Option<Vec<u8>> {
        self.get(key).filter(|bytes| bytes.len() <= max_bytes)
    }
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
    /// The project folder for disk-backed operations.
    fn folder(&self) -> Option<&Path> {
        None
    }
    fn record_shape_result(&self, _stock: &str, _asset: &str) -> Result<()> {
        Err("The project cannot record a saved shape.".into())
    }
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
    /// Saves parts that a design source gives again. The undo steps stay as they are.
    fn save_regenerated(&self, _expected: &ProjectData, _data: &ProjectData) -> Result<()> {
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
