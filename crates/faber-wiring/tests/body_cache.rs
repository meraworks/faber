//! Placement changes reuse the unplaced body of each part.

use faber_core::Project;
use faber_files::FileStore;
use faber_kernel_occt::OcctKernel;
use faber_model::{Edit, Geometry, Placement, Scalar, SectionAnalysis};
use faber_ports::{
    Combine, EdgeFace, EdgeInfo, FaceInfo, FaceSource, Kernel, Mesh, Properties, Result, Shape,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// Two parts with the same history. Part p depends on w. Part q does not.
const PROJECT: &str = r#"{
  "schema": 1,
  "revision": 0,
  "name": "Body cache",
  "params": { "w": "20" },
  "parts": {
    "p": {
      "name": "Part p",
      "colour": [10, 20, 30],
      "geometry": {
        "op": "history",
        "output": "sum",
        "features": [
          { "id": "a", "geometry": { "op": "box", "size": ["w", 10, 10] } },
          { "id": "b", "geometry": { "op": "box", "size": [5, 5, 5] } },
          { "id": "sum", "geometry": { "op": "union", "left": "a", "right": "b" } }
        ]
      }
    },
    "q": {
      "name": "Part q",
      "colour": [10, 20, 30],
      "geometry": {
        "op": "history",
        "output": "sum",
        "features": [
          { "id": "a", "geometry": { "op": "box", "size": [8, 8, 8] } },
          { "id": "b", "geometry": { "op": "box", "size": [4, 4, 4] } },
          { "id": "sum", "geometry": { "op": "union", "left": "a", "right": "b" } }
        ]
      }
    }
  }
}"#;

/// The exact kernel with counters for the calls that a part build makes.
struct Counting {
    inner: OcctKernel,
    tessellate: AtomicUsize,
    edge_lines: AtomicUsize,
    properties: AtomicUsize,
    faces: AtomicUsize,
}

impl Counting {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: OcctKernel,
            tessellate: AtomicUsize::new(0),
            edge_lines: AtomicUsize::new(0),
            properties: AtomicUsize::new(0),
            faces: AtomicUsize::new(0),
        })
    }
}

impl Kernel for Counting {
    fn face_sources(&self, shape: &Shape, inputs: &[Shape]) -> Result<Option<Vec<FaceSource>>> {
        self.inner.face_sources(shape, inputs)
    }
    fn edge_faces(&self, shape: &Shape) -> Result<Vec<EdgeFace>> {
        self.inner.edge_faces(shape)
    }
    fn faces(&self, shape: &Shape) -> Result<Vec<FaceInfo>> {
        self.faces.fetch_add(1, Ordering::Relaxed);
        self.inner.faces(shape)
    }
    fn edges(&self, shape: &Shape) -> Result<Vec<EdgeInfo>> {
        self.inner.edges(shape)
    }
    fn combine(&self, left: &Shape, right: &Shape, operation: Combine) -> Result<Shape> {
        self.inner.combine(left, right, operation)
    }
    fn transform_shape(&self, shape: &Shape, placement: &Placement) -> Result<Shape> {
        self.inner.transform_shape(shape, placement)
    }
    fn properties(&self, shape: &Shape) -> Result<Properties> {
        self.properties.fetch_add(1, Ordering::Relaxed);
        self.inner.properties(shape)
    }
    fn summary_properties(&self, shape: &Shape) -> Result<Properties> {
        self.inner.summary_properties(shape)
    }
    fn tessellate(&self, shape: &Shape, tolerance: f64) -> Result<faber_ports::Mesh> {
        Ok(self.tessellate_faces(shape, tolerance)?.mesh)
    }
    fn tessellate_faces(&self, shape: &Shape, tolerance: f64) -> Result<faber_ports::FaceMesh> {
        self.tessellate.fetch_add(1, Ordering::Relaxed);
        self.inner.tessellate_faces(shape, tolerance)
    }
    fn tessellate_edges(&self, shape: &Shape, tolerance: f64) -> Result<faber_ports::EdgeLines> {
        self.edge_lines.fetch_add(1, Ordering::Relaxed);
        self.inner.tessellate_edges(shape, tolerance)
    }
    fn build(&self, geometry: &Geometry) -> Result<Mesh> {
        self.inner.build(geometry)
    }
    fn exact_shape(&self, geometry: &Geometry) -> Result<Option<Shape>> {
        self.inner.exact_shape(geometry)
    }
    fn box_shape(&self, size: [f64; 3]) -> Result<Shape> {
        self.inner.box_shape(size)
    }
    fn cache_version(&self) -> Option<&'static str> {
        self.inner.cache_version()
    }
    fn face_count(&self, shape: &Shape) -> Result<u32> {
        self.inner.face_count(shape)
    }
}

fn open(root: &Path, kernel: &Arc<Counting>) -> Project {
    let store = Arc::new(FileStore::open(root).unwrap());
    let kernel: Arc<dyn Kernel> = kernel.clone();
    Project::open(store, kernel).unwrap()
}

fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), PROJECT).unwrap();
    dir
}

fn body<'a>(project: &'a Project, id: &str) -> &'a faber_ports::Body {
    project.bodies.iter().find(|body| body.id == id).unwrap()
}

fn number(value: f64) -> Scalar {
    Scalar::Number(value)
}

/// Sets the placement of a part. Translation is in mm and rotation in degrees.
fn place(project: &mut Project, id: &str, translation: [f64; 3], rotation: [f64; 3]) {
    let mut part = project.data.parts[id].clone();
    part.placement = Placement {
        translation: translation.map(number),
        rotation: rotation.map(number),
    };
    project.put_part(id.into(), part).unwrap();
}

fn translation_of(project: &Project, id: &str) -> [f64; 3] {
    project.data.parts[id]
        .placement
        .translation
        .clone()
        .map(|value| match value {
            Scalar::Number(value) => value,
            Scalar::Expression(_) => panic!("placement is an expression"),
        })
}

fn assert_bounds_near(actual: [[f64; 3]; 2], expected: [[f64; 3]; 2], tolerance: f64) {
    for corner in 0..2 {
        for axis in 0..3 {
            let difference = (actual[corner][axis] - expected[corner][axis]).abs();
            assert!(
                difference < tolerance,
                "bound {corner}.{axis}: {} and {}",
                actual[corner][axis],
                expected[corner][axis]
            );
        }
    }
}

#[test]
fn placement_change_does_not_tessellate() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let tessellated = kernel.tessellate.load(Ordering::Relaxed);
    let vertices = body(&project, "p").mesh.vertices.len();

    place(&mut project, "p", [30., -4., 2.], [0., 0., 0.]);

    assert_eq!(kernel.tessellate.load(Ordering::Relaxed), tessellated);
    let moved = body(&project, "p");
    assert_eq!(moved.mesh.vertices.len(), vertices);
    let fresh = faber_core::build(&project.data, &OcctKernel).unwrap();
    let fresh = fresh.iter().find(|body| body.id == "p").unwrap();
    assert_bounds_near(moved.bounds, fresh.bounds, 1e-6);
}

#[test]
fn thousand_moves_do_not_drift() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let start = body(&project, "p").bounds;
    for _ in 0..1000 {
        let [x, y, z] = translation_of(&project, "p");
        place(&mut project, "p", [x + 0.1, y, z], [0., 0., 0.]);
    }
    let [x, y, z] = translation_of(&project, "p");
    place(&mut project, "p", [x - 100., y, z], [0., 0., 0.]);

    assert_bounds_near(body(&project, "p").bounds, start, 1e-9);
}

#[test]
fn rotation_rotates_normals() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let unplaced = body(&project, "p").mesh.normals.clone();
    assert!(!unplaced.is_empty(), "the mesh has no normals");

    place(&mut project, "p", [0., 0., 0.], [0., 0., 90.]);

    let (sin, cos) = 90f64.to_radians().sin_cos();
    let turned = &body(&project, "p").mesh.normals;
    assert_eq!(turned.len(), unplaced.len());
    for (after, [x, y, z]) in turned.iter().zip(&unplaced) {
        let expected = [x * cos - y * sin, x * sin + y * cos, *z];
        for axis in 0..3 {
            assert!(
                (after[axis] - expected[axis]).abs() < 1e-12,
                "normal axis {axis}: {} and {}",
                after[axis],
                expected[axis]
            );
        }
    }
}

/// One cylinder. Its normals point off the axes.
const CYLINDER: &str = r#"{
  "schema": 1,
  "revision": 0,
  "name": "Cylinder",
  "params": {},
  "parts": {
    "c": {
      "name": "Cylinder",
      "colour": [10, 20, 30],
      "geometry": {
        "op": "history",
        "output": "c",
        "features": [
          { "id": "c", "geometry": { "op": "cylinder", "radius": 5, "height": 20 } }
        ]
      }
    }
  }
}"#;

fn cylinder_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), CYLINDER).unwrap();
    dir
}

#[test]
fn turned_bounds_match_a_fresh_build_and_the_kernel() {
    for rotation in [
        [90., 0., 0.],
        [0., 90., 0.],
        [0., 0., 90.],
        [90., 90., 0.],
        [30., 0., 45.],
    ] {
        let dir = cylinder_dir();
        let kernel = Counting::new();
        let mut project = open(dir.path(), &kernel);
        place(&mut project, "c", [3., -2., 7.], rotation);

        let turned = body(&project, "c");
        let fresh = faber_core::build(&project.data, &OcctKernel).unwrap();
        let fresh = fresh.iter().find(|body| body.id == "c").unwrap();
        assert_bounds_near(turned.bounds, fresh.bounds, 1e-6);
        let exact = OcctKernel
            .properties(turned.shape.as_ref().unwrap())
            .unwrap()
            .bounds;
        assert_bounds_near(turned.bounds, exact, 1e-6);
    }
}

#[test]
fn normals_turn_with_an_off_axis_placement() {
    let dir = cylinder_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let unplaced = body(&project, "c").mesh.normals.clone();
    assert!(
        unplaced
            .iter()
            .any(|n| n[0].abs() > 0.1 && n[1].abs() > 0.1),
        "no normal is off the axes"
    );

    let rotation = [30., 0., 45.];
    place(&mut project, "c", [0., 0., 0.], rotation);

    let matrix = glam::DMat3::from_rotation_z(45f64.to_radians())
        * glam::DMat3::from_rotation_y(0.)
        * glam::DMat3::from_rotation_x(30f64.to_radians());
    let turned = &body(&project, "c").mesh.normals;
    assert_eq!(turned.len(), unplaced.len());
    for (after, before) in turned.iter().zip(&unplaced) {
        let expected = (matrix * glam::DVec3::from(*before)).to_array();
        for (actual, wanted) in after.iter().zip(expected) {
            assert!(
                (actual - wanted).abs() < 1e-12,
                "normal {actual} and {wanted}"
            );
        }
    }
}

#[test]
fn geometry_change_tessellates_once() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let tessellated = kernel.tessellate.load(Ordering::Relaxed);

    project
        .set_params(BTreeMap::from([("w".to_string(), "30".to_string())]))
        .unwrap();

    // Only part p depends on w, so only part p tessellates again.
    assert_eq!(kernel.tessellate.load(Ordering::Relaxed), tessellated + 1);
}

/// A section that cuts the top face of part p. It stays the same in each test.
fn face_section(project: &mut Project) {
    let section: SectionAnalysis<Scalar> = serde_json::from_value(json!({
        "plane": {
            "kind": "face",
            "part": "p",
            "face": { "normal": [0, 0, 1], "expect": 1, "tolerance": 0.000001 },
            "offset": -5
        }
    }))
    .unwrap();
    project
        .batch(vec![Edit::SetSection {
            id: "cut".into(),
            section,
        }])
        .unwrap();
}

/// Counts the face queries of one move of a part, with or without the section on part p.
fn faces_for_move(part: &str, with_section: bool) -> usize {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    if with_section {
        face_section(&mut project);
    }
    kernel.faces.store(0, Ordering::Relaxed);
    place(&mut project, part, [40., 0., 0.], [0., 0., 0.]);
    kernel.faces.load(Ordering::Relaxed)
}

#[test]
fn warm_open_reads_the_mesh_from_the_feature_cache() {
    let dir = project_dir();
    let cache: Arc<dyn faber_ports::FeatureCache> =
        Arc::new(faber_files::FileFeatureCache::new(dir.path()));
    let open_warm = |kernel: &Arc<Counting>| {
        let store = Arc::new(FileStore::open(dir.path()).unwrap());
        let kernel: Arc<dyn Kernel> = kernel.clone();
        Project::open_with_cache(store, kernel, None, Some(cache.clone())).unwrap()
    };
    let cold = Counting::new();
    let first = open_warm(&cold);
    assert!(cold.tessellate.load(Ordering::Relaxed) > 0);
    let triangles = first.bodies[0].mesh.triangles.clone();
    let faces = first.bodies[0].face_of_triangle.clone();
    let edges = first.bodies[0].edge_lines.clone();
    assert!(!edges.is_empty());
    assert_eq!(faces.len(), triangles.len());
    drop(first);

    let warm = Counting::new();
    let second = open_warm(&warm);

    assert_eq!(warm.tessellate.load(Ordering::Relaxed), 0);
    assert_eq!(second.bodies[0].mesh.triangles, triangles);
    assert_eq!(second.bodies[0].face_of_triangle, faces);
    assert_eq!(second.bodies[0].edge_lines, edges);
    assert_eq!(warm.edge_lines.load(Ordering::Relaxed), 0);
}

#[test]
fn section_of_an_unchanged_part_is_not_recomputed() {
    assert_eq!(faces_for_move("q", true), faces_for_move("q", false));
}

#[test]
fn section_of_a_moved_cut_part_is_kept() {
    // A plane section reads no part. A move of part p keeps its result.
    let faces_for_move_with_plane = |with_section: bool| {
        let dir = project_dir();
        let kernel = Counting::new();
        let mut project = open(dir.path(), &kernel);
        if with_section {
            let section: SectionAnalysis<Scalar> = serde_json::from_value(json!({
                "plane": { "kind": "plane", "origin": [0, 0, 5], "normal": [0, 0, 1] }
            }))
            .unwrap();
            project
                .batch(vec![Edit::SetSection {
                    id: "cut".into(),
                    section,
                }])
                .unwrap();
        }
        kernel.faces.store(0, Ordering::Relaxed);
        place(&mut project, "p", [40., 0., 0.], [0., 0., 0.]);
        kernel.faces.load(Ordering::Relaxed)
    };
    assert_eq!(
        faces_for_move_with_plane(true),
        faces_for_move_with_plane(false)
    );
}

/// Counts the face queries of one change of a size, with or without the section on part p.
fn faces_for_resize(with_section: bool) -> usize {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    if with_section {
        face_section(&mut project);
    }
    kernel.faces.store(0, Ordering::Relaxed);
    project
        .set_params(BTreeMap::from([("w".to_string(), "30".to_string())]))
        .unwrap();
    kernel.faces.load(Ordering::Relaxed)
}

#[test]
fn section_of_a_resized_part_is_recomputed() {
    assert!(faces_for_resize(true) > faces_for_resize(false));
}

#[test]
fn turning_the_cut_part_of_a_face_section_is_refused() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    face_section(&mut project);

    // A 30 degree turn leaves no face with the selected normal.
    let mut part = project.data.parts["p"].clone();
    part.placement = Placement {
        translation: [number(0.), number(0.), number(0.)],
        rotation: [number(30.), number(0.), number(0.)],
    };
    let error = project.put_part("p".into(), part).unwrap_err();

    assert!(error.starts_with("Section cut:"), "{error}");
    assert_eq!(
        project.data.parts["p"].placement.rotation,
        [number(0.), number(0.), number(0.)]
    );
}

/// The creating feature of each face of a part, in face order.
fn created_by(project: &Project, id: &str) -> Vec<Option<Vec<String>>> {
    let (_, faces) = project.topology(id).unwrap();
    faces.into_iter().map(|face| face.created_by).collect()
}

#[test]
fn moved_part_keeps_the_feature_of_each_face() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let before = created_by(&project, "p");
    assert!(before.iter().any(Option::is_some), "no face has a feature");

    place(&mut project, "p", [30., -4., 2.], [0., 0., 90.]);

    assert_eq!(created_by(&project, "p"), before);
}

#[test]
fn edge_lines_move_with_the_part() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);
    let lines = body(&project, "p").edge_lines.clone();
    assert!(!lines.ranges.is_empty());
    let calls = kernel.edge_lines.load(Ordering::Relaxed);
    assert!(calls > 0);
    place(&mut project, "p", [3., 4., 5.], [0., 0., 90.]);
    let moved = &body(&project, "p").edge_lines;
    assert_eq!(moved.ranges, lines.ranges);
    assert_eq!(moved.points.len(), lines.points.len());
    for ([x, y, z], p) in lines.points.iter().zip(&moved.points) {
        assert!(glam::DVec3::new(3. - y, 4. + x, 5. + z).distance(glam::DVec3::from(*p)) < 1e-9);
    }
    assert_eq!(kernel.edge_lines.load(Ordering::Relaxed), calls);
}

#[test]
fn component_move_does_not_tessellate() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut p = open(dir.path(), &kernel);
    let mut source = p.data.parts["p"].clone();
    source.component = Some("frame".into());
    let component = serde_json::from_value(
        json!({"tool":"set_component","id":"frame","component":{"name":"Frame"}}),
    )
    .unwrap();
    p.batch(vec![
        component,
        Edit::PutPart {
            id: "p".into(),
            part: source,
        },
    ])
    .unwrap();
    let before = kernel.tessellate.load(Ordering::Relaxed);
    p.move_component("frame", &faber_core::Motion::Translate([4., 2., 0.]))
        .unwrap();
    assert_eq!(kernel.tessellate.load(Ordering::Relaxed), before);
}

#[test]
fn instances_share_one_tessellation() {
    let dir = project_dir();
    let mut data: serde_json::Value = serde_json::from_str(PROJECT).unwrap();
    let original = data["parts"]["p"].take();
    data["parts"] = json!({"source":original});
    for id in ["a", "b", "c", "d"] {
        data["parts"][id] =
            json!({"name":id,"colour":[10,20,30],"geometry":{"op":"instance","of":"source"}});
    }
    fs::write(
        dir.path().join("faber.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    let kernel = Counting::new();
    let p = open(dir.path(), &kernel);
    assert_eq!(p.bodies.len(), 5);
    assert_eq!(kernel.tessellate.load(Ordering::Relaxed), 1);
    for id in ["a", "b", "c", "d"] {
        assert!(
            p.local_geometry("source")
                .unwrap()
                .1
                .unwrap()
                .same_instance(p.local_geometry(id).unwrap().1.unwrap())
        );
    }
}
