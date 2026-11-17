use faber_clash::scene::Rules;
use faber_clash::{Part, PartKind, ScanOptions, Scene, Status, scan};
use parry3d_f64::{math::Vector, shape::TriMesh};

fn scene(gap: f64) -> Scene {
    let part = |name: &str, z: f64| Part {
        name: name.into(),
        kind: PartKind::Printed,
        movable: false,
        mesh: TriMesh::new(
            vec![
                Vector::new(0.0, 0.0, z),
                Vector::new(2.0, 0.0, z),
                Vector::new(0.0, 2.0, z),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap(),
    };
    Scene {
        signature: "test".into(),
        parts: vec![part("a", 0.0), part("b", gap)],
        poses: vec![],
        rules: Rules {
            allowed_contacts: Default::default(),
            allowed_interference: Default::default(),
            contact_limits_mm: Default::default(),
            excluded_pairs: Default::default(),
            proximity_mm: 0.1,
        },
    }
}

#[test]
fn separated_parts_pass() {
    let report = scan(&scene(2.0), &ScanOptions::default()).unwrap();
    assert_eq!(report.status, Status::Pass);
    assert_eq!(report.aabb_rejected, 1);
}

#[test]
fn scan_refuses_invalid_rules_names_and_pose_lengths() {
    for proximity in [-1., f64::NAN, f64::INFINITY] {
        let mut scene = scene(2.);
        scene.rules.proximity_mm = proximity;
        assert!(scan(&scene, &ScanOptions::default()).is_err());
    }
    let mut input = scene(2.);
    input.parts[1].name = input.parts[0].name.clone();
    assert!(scan(&input, &ScanOptions::default()).is_err());
    let mut input = scene(2.);
    input.poses.push(faber_clash::scene::PoseSet {
        label: vec![1.],
        transforms: vec![],
    });
    assert!(scan(&input, &ScanOptions::default()).is_err());
    let mut input = scene(2.);
    input
        .rules
        .allowed_interference
        .insert(Rules::key("a", "b"), -1.);
    assert!(scan(&input, &ScanOptions::default()).is_err());
}

#[test]
fn pose_matrices_refuse_scale_reflection_and_non_finite_values() {
    use faber_clash::scene::pose_from_matrix;
    let identity = vec![
        vec![1., 0., 0., 5.],
        vec![0., 1., 0., 6.],
        vec![0., 0., 1., 7.],
    ];
    assert!(pose_from_matrix(&identity).is_ok());
    for value in [2., -1., f64::NAN, f64::INFINITY] {
        let mut matrix = identity.clone();
        matrix[0][0] = value;
        assert!(pose_from_matrix(&matrix).is_err());
    }
}

fn cube(lo: f64, hi: f64) -> TriMesh {
    let vertices = vec![
        Vector::new(lo, lo, lo),
        Vector::new(hi, lo, lo),
        Vector::new(hi, hi, lo),
        Vector::new(lo, hi, lo),
        Vector::new(lo, lo, hi),
        Vector::new(hi, lo, hi),
        Vector::new(hi, hi, hi),
        Vector::new(lo, hi, hi),
    ];
    TriMesh::with_flags(
        vertices,
        vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
        parry3d_f64::shape::TriMeshFlags::ORIENTED,
    )
    .unwrap()
}

#[test]
fn full_containment_is_interference_without_surface_contact() {
    let mut scene = scene(0.);
    scene.parts[0].mesh = cube(0., 10.);
    scene.parts[1].mesh = cube(3., 7.);
    let report = scan(&scene, &ScanOptions::default()).unwrap();
    assert_eq!(report.status, Status::Fail);
    assert_eq!(report.violations.len(), 1);
    assert!(report.violations[0].depth_mm > 2.9);
}

#[test]
fn crossing_bars_are_not_allowed_contact_when_vertex_depth_is_zero() {
    let scaled_cube = |scale: Vector| {
        let mesh = cube(-1., 1.);
        TriMesh::with_flags(
            mesh.vertices().iter().map(|p| *p * scale).collect(),
            mesh.indices().to_vec(),
            parry3d_f64::shape::TriMeshFlags::ORIENTED,
        )
        .unwrap()
    };
    let mut input = scene(0.);
    input.parts[0].mesh = scaled_cube(Vector::new(5., 1., 1.));
    input.parts[1].mesh = scaled_cube(Vector::new(1., 5., 1.));
    input.rules.allowed_contacts.insert(Rules::key("a", "b"));
    let report = scan(&input, &ScanOptions::default()).unwrap();
    assert_eq!(report.status, Status::Fail);
    assert_eq!(report.violations.len(), 1);
    input
        .rules
        .allowed_interference
        .insert(Rules::key("a", "b"), 0.1);
    assert!(report.violations[0].surface_crossing);
    assert!(report.violations[0].depth_mm <= 1e-9);
    assert_eq!(
        scan(&input, &ScanOptions::default()).unwrap().status,
        Status::Fail
    );
}

#[test]
fn touching_closed_faces_can_still_be_allowed() {
    let mut input = scene(0.);
    input.parts[0].mesh = cube(0., 2.);
    let other = cube(0., 2.);
    input.parts[1].mesh = TriMesh::with_flags(
        other
            .vertices()
            .iter()
            .map(|p| *p + Vector::new(2., 0., 0.))
            .collect(),
        other.indices().to_vec(),
        parry3d_f64::shape::TriMeshFlags::ORIENTED,
    )
    .unwrap();
    input.rules.allowed_contacts.insert(Rules::key("a", "b"));
    assert_eq!(
        scan(&input, &ScanOptions::default()).unwrap().status,
        Status::Pass
    );
}

#[test]
fn small_gap_is_reported_and_can_be_allowed() {
    let mut scene = scene(0.05);
    let report = scan(&scene, &ScanOptions::default()).unwrap();
    assert_eq!(report.status, Status::Fail);
    assert_eq!(report.violations.len(), 1);
    assert!((report.violations[0].gap_mm - 0.05).abs() < 1e-10);
    scene.rules.allowed_contacts.insert(Rules::key("a", "b"));
    let report = scan(&scene, &ScanOptions::default()).unwrap();
    assert_eq!(report.status, Status::Pass);
    assert_eq!(report.allowed_contacts.len(), 1);
}

#[test]
fn scan_does_not_set_the_global_worker_pool() {
    scan(
        &scene(2.0),
        &ScanOptions {
            static_only: true,
            threads: 2,
        },
    )
    .unwrap();
    assert!(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build_global()
            .is_ok()
    );
}
