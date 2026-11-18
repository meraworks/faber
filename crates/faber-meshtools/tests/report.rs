mod common;
use common::{append, cube};
use faber_meshtools::{report, try_report};
use faber_ports::Mesh;
#[test]
fn clean_cube() {
    let r = report(&cube(10., 1));
    assert_eq!((r.vertices, r.triangles, r.shells), (8, 12, 1));
    assert!(r.watertight);
    assert!((r.volume_mm3.unwrap() - 1000.).abs() < 1e-9);
    assert_eq!(r.area_mm2, 600.);
    assert!(r.problems.is_empty());
}
#[test]
fn cube_with_a_missing_face() {
    let mut m = cube(10., 1);
    m.triangles.truncate(10);
    let r = report(&m);
    assert_eq!(r.open_edges, 4);
    assert!(!r.watertight && r.volume_mm3.is_none());
    assert!(r.problems.iter().any(|p| p.contains("4 open edges")));
}
#[test]
fn cube_with_one_flipped_triangle() {
    let mut m = cube(10., 1);
    m.triangles[5].swap(1, 2);
    assert_eq!(report(&m).flipped, 1);
}
#[test]
fn unwelded_cube_is_welded_first() {
    let b = cube(10., 1);
    let mut m = Mesh::default();
    m.vertices.extend(b.triangle_vertices());
    m.triangles = (0..12).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect();
    let r = report(&m);
    assert_eq!(r.vertices, 8);
    assert!(r.watertight);
    assert_eq!(r.shells, 1);
}
#[test]
fn shared_edge_of_two_cubes() {
    let mut m = cube(10., 1);
    append(&mut m, &cube(10., 1), [10., 10., 0.]);
    let r = report(&m);
    assert_eq!((r.non_manifold_edges, r.shells), (1, 2));
    assert!(!r.watertight);
}
#[test]
fn touching_at_a_single_vertex_is_not_a_manifold_surface() {
    let mut m = cube(10., 1);
    append(&mut m, &cube(10., 1), [10., 10., 10.]);
    let r = report(&m);
    assert_eq!(r.non_manifold_edges, 0);
    assert_eq!(r.shells, 2);
    assert!(!r.watertight);
    assert!(r.problems.iter().any(|p| p.contains("non-manifold vert")));
}
#[test]
fn degenerate_triangle_is_counted() {
    let mut m = cube(10., 1);
    m.triangles.push([0, 0, 1]);
    assert_eq!(report(&m).degenerate, 1);
}
#[test]
fn malformed_mesh_is_reported() {
    let mut m = cube(10., 1);
    m.triangles[0][0] = u32::MAX;
    assert!(try_report(&m).is_err());
    let r = report(&m);
    assert!(!r.watertight && !r.problems.is_empty());
}
