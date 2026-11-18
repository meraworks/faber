use faber_model::Edit;
use serde_json::json;

fn edits(value: serde_json::Value) -> Vec<Edit> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn conditional_dimensions_rebuild_when_a_parameter_crosses_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Conditional".into())).unwrap();
    project.batch(edits(json!([
        {"tool":"set_param","name":"width","expression":"20"},
        {"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":["width",10,"if(width >= 30, 4, 2)"]}}}
    ]))).unwrap();
    assert!((project.bodies[0].volume - 400.).abs() < 1e-6);
    project
        .set_params([("width".into(), "30".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 1200.).abs() < 1e-6);
    project.undo().unwrap();
    assert!((project.bodies[0].volume - 400.).abs() < 1e-6);
}

#[test]
fn mixed_changes_build_once_save_once_and_undo_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Batch".into())).unwrap();
    project.batch(edits(json!([
        {"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"history","features":[{"id":"base","geometry":{"op":"box","size":[10,20,"height"]}}],"output":"base"}}},
        {"tool":"set_param","name":"height","expression":"30"},
        {"tool":"edit_feature","part":"box","id":"base","geometry":{"op":"box","size":[20,20,"height"]}}
    ]))).unwrap();
    assert_eq!(project.data.revision, 1);
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-6);
    project.undo().unwrap();
    assert!(project.data.parts.is_empty());
    assert!(project.data.params.is_empty());
    project.redo().unwrap();
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-6);
}

#[test]
fn failed_batch_keeps_file_memory_and_history_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Batch".into())).unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    for operations in [
        json!([{"tool":"set_param","name":"height","expression":"30"},{"tool":"remove_part","id":"missing"}]),
        json!([{"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,-1]}}}]),
    ] {
        assert!(project.batch(edits(operations)).is_err());
        assert_eq!(
            faber_wiring::project_fingerprint(dir.path()).unwrap(),
            before
        );
        assert!(project.data.parts.is_empty());
        assert!(project.data.params.is_empty());
        assert!(project.undo().is_err());
    }
    assert!(
        serde_json::from_value::<Vec<Edit>>(json!([{"tool":"export","directory":"out"}])).is_err()
    );
}

fn operation(value: serde_json::Value) -> faber_core::Operation {
    faber_core::Operation::from(serde_json::from_value::<Edit>(value).unwrap())
}

fn block(size: [f64; 3]) -> serde_json::Value {
    json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"box","size":size}})
}

fn part(value: serde_json::Value) -> faber_model::Part {
    serde_json::from_value(value).unwrap()
}

#[test]
fn assembly_in_one_batch() {
    use faber_core::{AlignMode, Axis, Motion, Operation, Rotation, RotationAxis};
    let turn = Motion::Rotate(Rotation {
        axis: RotationAxis::Named(Axis::Z),
        angle_deg: 90.,
        about: None,
    });
    let contact = json!({"tool":"set_contact","a":"block","b":"base","allowed":true});
    let dir = tempfile::tempdir().unwrap();
    let mut batched = faber_wiring::open(&dir.path().join("batched"), true, None).unwrap();
    batched
        .batch_operations(
            vec![
                operation(json!({"tool":"put_part","id":"base","part":block([40.,40.,10.])})),
                operation(json!({"tool":"put_part","id":"block","part":block([10.,10.,30.])})),
                operation(json!({"tool":"put_part","id":"cap","part":block([5.,5.,5.])})),
                Operation::Align {
                    id: "block".into(),
                    target: "base".into(),
                    axes: vec![Axis::X, Axis::Y],
                    mode: AlignMode::Centre,
                    gap_mm: 0.,
                },
                Operation::Align {
                    id: "cap".into(),
                    target: "block".into(),
                    axes: vec![Axis::Z],
                    mode: AlignMode::MinToMax,
                    gap_mm: 0.,
                },
                Operation::Move {
                    id: "cap".into(),
                    motion: turn.clone(),
                    copy_as: None,
                },
                operation(contact.clone()),
            ],
            false,
        )
        .unwrap();
    assert_eq!(batched.data.revision, 1);

    let mut single = faber_wiring::open(&dir.path().join("single"), true, None).unwrap();
    single
        .put_part("base".into(), part(block([40., 40., 10.])))
        .unwrap();
    single
        .put_part("block".into(), part(block([10., 10., 30.])))
        .unwrap();
    single
        .put_part("cap".into(), part(block([5., 5., 5.])))
        .unwrap();
    single
        .align_part("block", "base", &[Axis::X, Axis::Y], AlignMode::Centre, 0.)
        .unwrap();
    single
        .align_part("cap", "block", &[Axis::Z], AlignMode::MinToMax, 0.)
        .unwrap();
    single.move_part("cap", turn, None).unwrap();
    single
        .batch(vec![serde_json::from_value(contact).unwrap()])
        .unwrap();
    assert_eq!(single.data.revision, 7);
    for id in ["base", "block", "cap"] {
        assert_eq!(
            batched.data.parts[id].placement, single.data.parts[id].placement,
            "{id}"
        );
    }
}

#[test]
fn batch_move_uses_the_new_bounds() {
    use faber_core::{AlignMode, Axis, Operation};
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Bounds".into())).unwrap();
    let base = json!({"name":"Base","colour":[100,140,170],"geometry":{"op":"history","features":[{"id":"base","geometry":{"op":"box","size":[10,10,10]}}],"output":"base"}});
    project
        .batch(edits(json!([
            {"tool":"put_part","id":"base","part":base},
            {"tool":"put_part","id":"top","part":block([10.,10.,5.])}
        ])))
        .unwrap();
    project
        .batch_operations(
            vec![
                operation(json!({"tool":"edit_feature","part":"base","id":"base","geometry":{"op":"box","size":[10,10,20]}})),
                Operation::Align {
                    id: "top".into(),
                    target: "base".into(),
                    axes: vec![Axis::Z],
                    mode: AlignMode::MinToMax,
                    gap_mm: 0.,
                },
            ],
            false,
        )
        .unwrap();
    let top = project.bodies.iter().find(|body| body.id == "top").unwrap();
    assert!((top.bounds[0][2] - 20.).abs() < 1e-9, "{:?}", top.bounds);
}

/// The exact kernel with a count of tessellations.
struct Tessellating {
    inner: faber_kernel_occt::OcctKernel,
    tessellations: std::sync::atomic::AtomicUsize,
}

impl Tessellating {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: faber_kernel_occt::OcctKernel,
            tessellations: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

impl faber_ports::Kernel for Tessellating {
    fn face_sources(
        &self,
        shape: &faber_ports::Shape,
        inputs: &[faber_ports::Shape],
    ) -> faber_ports::Result<Option<Vec<faber_ports::FaceSource>>> {
        self.inner.face_sources(shape, inputs)
    }
    fn edge_faces(
        &self,
        shape: &faber_ports::Shape,
    ) -> faber_ports::Result<Vec<faber_ports::EdgeFace>> {
        self.inner.edge_faces(shape)
    }
    fn faces(&self, shape: &faber_ports::Shape) -> faber_ports::Result<Vec<faber_ports::FaceInfo>> {
        self.inner.faces(shape)
    }
    fn edges(&self, shape: &faber_ports::Shape) -> faber_ports::Result<Vec<faber_ports::EdgeInfo>> {
        self.inner.edges(shape)
    }
    fn combine(
        &self,
        left: &faber_ports::Shape,
        right: &faber_ports::Shape,
        operation: faber_ports::Combine,
    ) -> faber_ports::Result<faber_ports::Shape> {
        self.inner.combine(left, right, operation)
    }
    fn transform_shape(
        &self,
        shape: &faber_ports::Shape,
        placement: &faber_model::Placement,
    ) -> faber_ports::Result<faber_ports::Shape> {
        self.inner.transform_shape(shape, placement)
    }
    fn properties(
        &self,
        shape: &faber_ports::Shape,
    ) -> faber_ports::Result<faber_ports::Properties> {
        self.inner.properties(shape)
    }
    fn summary_properties(
        &self,
        shape: &faber_ports::Shape,
    ) -> faber_ports::Result<faber_ports::Properties> {
        self.inner.summary_properties(shape)
    }
    fn tessellate(
        &self,
        shape: &faber_ports::Shape,
        tolerance: f64,
    ) -> faber_ports::Result<faber_ports::Mesh> {
        self.tessellations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner.tessellate(shape, tolerance)
    }
    fn build(&self, geometry: &faber_model::Geometry) -> faber_ports::Result<faber_ports::Mesh> {
        self.inner.build(geometry)
    }
    fn exact_shape(
        &self,
        geometry: &faber_model::Geometry,
    ) -> faber_ports::Result<Option<faber_ports::Shape>> {
        self.inner.exact_shape(geometry)
    }
    fn box_shape(&self, size: [f64; 3]) -> faber_ports::Result<faber_ports::Shape> {
        self.inner.box_shape(size)
    }
    fn cache_version(&self) -> Option<&'static str> {
        self.inner.cache_version()
    }
    fn face_count(&self, shape: &faber_ports::Shape) -> faber_ports::Result<u32> {
        self.inner.face_count(shape)
    }
}

#[test]
fn align_batch_places_unchanged_parts_without_tessellating() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Align".into())).unwrap();
    let puts: Vec<faber_core::Operation> = (0..20u32)
        .map(|i| {
            let x = f64::from(i) * 20.;
            operation(json!({"tool":"put_part","id":format!("p{i}"),"part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]},"placement":{"translation":[x,0,15]}}}))
        })
        .collect();
    project.batch_operations(puts, false).unwrap();
    drop(project);

    let kernel = Tessellating::new();
    let store = std::sync::Arc::new(faber_files::FileStore::open(dir.path()).unwrap());
    let mut project = faber_core::Project::open(store, kernel.clone()).unwrap();
    kernel
        .tessellations
        .store(0, std::sync::atomic::Ordering::Relaxed);
    let aligns: Vec<faber_core::Operation> = (0..20u32)
        .map(|i| faber_core::Operation::Align {
            id: format!("p{i}"),
            target: "origin".into(),
            axes: vec![faber_core::Axis::Z],
            mode: faber_core::AlignMode::Min,
            gap_mm: 0.,
        })
        .collect();
    project.batch_operations(aligns, false).unwrap();
    assert_eq!(
        kernel
            .tessellations
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    for i in 0..20u32 {
        let id = format!("p{i}");
        let body = project.bodies.iter().find(|body| body.id == id).unwrap();
        assert!(body.bounds[0][2].abs() < 1e-9, "{id}: {:?}", body.bounds);
    }
}

fn centre_of(project: &faber_core::Project, id: &str) -> [f64; 3] {
    let body = project.bodies.iter().find(|body| body.id == id).unwrap();
    [0, 1, 2].map(|axis| (body.bounds[0][axis] + body.bounds[1][axis]) / 2.)
}

#[test]
fn group_move_is_one_revision_and_keeps_the_spacing() {
    use faber_core::Motion;
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Group".into())).unwrap();
    project
        .batch(edits(json!([
            {"tool":"put_part","id":"a","part":block([10.,10.,10.])},
            {"tool":"put_part","id":"b","part":block([10.,10.,10.])}
        ])))
        .unwrap();
    project
        .move_parts(&["b".into()], Motion::Translate([40., 0., 0.]))
        .unwrap();
    let revision = project.data.revision;
    // Centres: a at (5, 5, 5) and b at (45, 5, 5). The common centre is (25, 5, 5).
    let rotation = serde_json::from_value(json!({"axis": "z", "angle_deg": 90})).unwrap();
    project
        .move_parts(&["a".into(), "b".into()], Motion::Rotate(rotation))
        .unwrap();
    assert_eq!(project.data.revision, revision + 1);
    let a = centre_of(&project, "a");
    let b = centre_of(&project, "b");
    assert!(
        (a[0] - 25.).abs() < 1e-6 && (a[1] + 15.).abs() < 1e-6,
        "{a:?}"
    );
    assert!(
        (b[0] - 25.).abs() < 1e-6 && (b[1] - 25.).abs() < 1e-6,
        "{b:?}"
    );
    let gap = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    assert!((gap - 40.).abs() < 1e-6, "{gap}");
    project.undo().unwrap();
    assert_eq!(centre_of(&project, "a"), [5., 5., 5.]);
    assert_eq!(centre_of(&project, "b"), [45., 5., 5.]);
}
