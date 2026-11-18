mod common;
use common::{cube, signed, sphere};
use faber_meshtools::{Target, reduce, report};
use glam::DVec3;
#[test]
fn sphere_to_a_tenth() {
    let m = sphere(10., 100, 100);
    assert_eq!(m.triangles.len(), 20_000);
    let (f, l) = reduce(&m, Target::Triangles(2000)).unwrap();
    assert!(l.after <= 2000 && l.after == f.triangles.len());
    assert!(report(&f).watertight);
    assert!((signed(&f) / signed(&m) - 1.).abs() < 0.02);
    assert!(l.max_error_mm < 0.2, "{}", l.max_error_mm);
}
#[test]
fn flat_faces_collapse_fully() {
    let m = cube(10., 10);
    assert_eq!(m.triangles.len(), 1200);
    let (f, l) = reduce(&m, Target::MaxError(0.001)).unwrap();
    assert_eq!(l.after, 12);
    assert!((signed(&f) - 1000.).abs() < 1e-6);
    assert!(l.max_error_mm < 1e-6);
    assert!(report(&f).watertight);
}
#[test]
fn sharp_edges_stay() {
    let (f, _) = reduce(&cube(10., 10), Target::Triangles(12)).unwrap();
    for x in [0., 10.] {
        for y in [0., 10.] {
            for z in [0., 10.] {
                assert!(
                    f.vertices
                        .iter()
                        .any(|p| DVec3::from(*p).distance(DVec3::new(x, y, z)) < 1e-9)
                );
            }
        }
    }
}
#[test]
fn target_above_the_count_is_a_no_op() {
    let m = cube(10., 1);
    let (f, l) = reduce(&m, Target::Triangles(50)).unwrap();
    assert!(!l.changed);
    assert_eq!(l.before, l.after);
    assert_eq!(m.triangles, f.triangles);
    assert_eq!(m.vertices, f.vertices);
}
#[test]
fn open_mesh_is_refused() {
    let mut m = cube(10., 1);
    m.triangles.pop();
    assert!(reduce(&m, Target::Ratio(0.5)).is_err());
}
#[test]
fn excessive_reduction_names_the_input_and_work_limit() {
    let m = faber_ports::Mesh {
        vertices: vec![[0.; 3]],
        triangles: vec![[0, 0, 0]; 2_000_001],
        normals: vec![],
    };
    let e = reduce(&m, Target::Triangles(1_000)).unwrap_err();
    assert!(e.contains("2000001") && e.contains("2000000"), "{e}");
}
#[test]
fn max_error_is_a_bound_on_the_result() {
    let m = sphere(10., 24, 12);
    let (f, l) = reduce(&m, Target::MaxError(0.035)).unwrap();
    let b = faber_geometry::bvh::Bvh::new(&f).unwrap();
    let actual = m
        .vertices
        .iter()
        .map(|&p| b.closest(p).t)
        .fold(0., f64::max);
    assert!(actual <= 0.035 + 1e-9);
    assert!((actual - l.max_error_mm).abs() < 1e-9);
}
