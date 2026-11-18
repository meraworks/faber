//! Read results are kept until the revision changes.

use faber_core::{Project, TopologyKind};
use faber_files::FileStore;
use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{
    Combine, EdgeInfo, FaceInfo, Kernel, MassProperties, Mesh, Properties, Result, Shape,
};
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const PROJECT: &str = r#"{
  "schema": 1,
  "revision": 0,
  "name": "Read cache",
  "params": {},
  "parts": {
    "p": {
      "name": "Part p",
      "colour": [10, 20, 30],
      "geometry": { "op": "box", "size": [20, 10, 10] }
    }
  },
  "sections": {
    "mid": { "plane": { "kind": "plane", "origin": [0, 0, 5], "normal": [0, 0, 1] } }
  }
}"#;

/// The exact kernel with counters. `calls` counts every call of a read.
struct Counting {
    inner: OcctKernel,
    calls: AtomicUsize,
    faces: AtomicUsize,
    edges: AtomicUsize,
    mass: AtomicUsize,
}

impl Counting {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: OcctKernel,
            calls: AtomicUsize::new(0),
            faces: AtomicUsize::new(0),
            edges: AtomicUsize::new(0),
            mass: AtomicUsize::new(0),
        })
    }

    fn count(&self, counter: &AtomicUsize) {
        counter.fetch_add(1, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

impl Kernel for Counting {
    fn face_sources(
        &self,
        shape: &Shape,
        inputs: &[Shape],
    ) -> Result<Option<Vec<faber_ports::FaceSource>>> {
        self.inner.face_sources(shape, inputs)
    }
    fn edge_faces(&self, shape: &Shape) -> Result<Vec<faber_ports::EdgeFace>> {
        self.count(&self.calls);
        self.inner.edge_faces(shape)
    }
    fn split(
        &self,
        shape: &Shape,
        origin: [f64; 3],
        normal: [f64; 3],
        keep: faber_model::SplitKeep,
    ) -> Result<Shape> {
        self.count(&self.calls);
        self.inner.split(shape, origin, normal, keep)
    }
    fn face_shape(&self, shape: &Shape, index: u32) -> Result<Shape> {
        self.count(&self.calls);
        self.inner.face_shape(shape, index)
    }
    fn mass_properties(&self, shape: &Shape) -> Result<MassProperties> {
        self.count(&self.mass);
        self.inner.mass_properties(shape)
    }
    fn faces(&self, shape: &Shape) -> Result<Vec<FaceInfo>> {
        self.count(&self.faces);
        self.inner.faces(shape)
    }
    fn edges(&self, shape: &Shape) -> Result<Vec<EdgeInfo>> {
        self.count(&self.edges);
        self.inner.edges(shape)
    }
    fn combine(&self, left: &Shape, right: &Shape, operation: Combine) -> Result<Shape> {
        self.inner.combine(left, right, operation)
    }
    fn transform_shape(&self, shape: &Shape, placement: &Placement) -> Result<Shape> {
        self.inner.transform_shape(shape, placement)
    }
    fn properties(&self, shape: &Shape) -> Result<Properties> {
        self.count(&self.calls);
        self.inner.properties(shape)
    }
    fn summary_properties(&self, shape: &Shape) -> Result<Properties> {
        self.inner.summary_properties(shape)
    }
    fn tessellate(&self, shape: &Shape, tolerance: f64) -> Result<Mesh> {
        self.inner.tessellate(shape, tolerance)
    }
    fn build(&self, geometry: &faber_model::Geometry) -> Result<Mesh> {
        self.inner.build(geometry)
    }
    fn exact_shape(&self, geometry: &faber_model::Geometry) -> Result<Option<Shape>> {
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

fn open(root: &std::path::Path, kernel: &Arc<Counting>) -> Project {
    let store = Arc::new(FileStore::open(root).unwrap());
    let kernel: Arc<dyn Kernel> = kernel.clone();
    Project::open(store, kernel).unwrap()
}

fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), PROJECT).unwrap();
    dir
}

#[test]
fn second_topology_call_uses_the_cache() {
    let dir = project_dir();
    let kernel = Counting::new();
    let project = open(dir.path(), &kernel);
    project.selected_topology("p", None, None, None).unwrap();
    let faces = kernel.faces.load(Ordering::Relaxed);
    let edges = kernel.edges.load(Ordering::Relaxed);
    assert!(faces > 0 && edges > 0);

    let (edge_list, face_list) = project.selected_topology("p", None, None, None).unwrap();

    assert_eq!(kernel.faces.load(Ordering::Relaxed), faces);
    assert_eq!(kernel.edges.load(Ordering::Relaxed), edges);
    assert_eq!(edge_list.len(), 12);
    assert_eq!(face_list.len(), 6);
}

#[test]
fn edge_kind_does_not_list_faces() {
    let dir = project_dir();
    let kernel = Counting::new();
    let project = open(dir.path(), &kernel);

    let (edges, faces) = project
        .selected_topology_kind("p", None, TopologyKind::Edges, None, None)
        .unwrap();

    assert_eq!(edges.map(|list| list.len()), Some(12));
    assert!(faces.is_none());
    assert_eq!(kernel.faces.load(Ordering::Relaxed), 0);
}

#[test]
fn mass_and_section_are_cached_per_revision() {
    let dir = project_dir();
    let kernel = Counting::new();
    let mut project = open(dir.path(), &kernel);

    let mass = project.mass_properties("p", Some(1.2)).unwrap();
    let mass_calls = kernel.mass.load(Ordering::Relaxed);
    let again = project.mass_properties("p", Some(1.2)).unwrap();
    assert_eq!(kernel.mass.load(Ordering::Relaxed), mass_calls);
    assert_eq!(mass.volume_mm3, again.volume_mm3);
    assert_eq!(mass.mass_g, again.mass_g);
    assert_eq!(mass.centre_of_mass_mm, again.centre_of_mass_mm);

    let section = project.section_info("mid").unwrap();
    let section_calls = kernel.calls.load(Ordering::Relaxed);
    let section_again = project.section_info("mid").unwrap();
    assert_eq!(kernel.calls.load(Ordering::Relaxed), section_calls);
    assert_eq!(section, section_again);

    // A save changes the revision and clears the cache.
    project
        .set_params(BTreeMap::from([("spare".to_string(), "1".to_string())]))
        .unwrap();
    project.mass_properties("p", Some(1.2)).unwrap();
    assert_eq!(kernel.mass.load(Ordering::Relaxed), mass_calls + 1);
    project.section_info("mid").unwrap();
    assert!(kernel.calls.load(Ordering::Relaxed) > section_calls);
}
