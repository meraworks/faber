use faber_meshtools::{Keep, cut, slice};
use faber_ports::Mesh;
use glam::DVec3;
use std::collections::BTreeMap;

const UP: [f64; 3] = [0., 0., 1.];

fn cube() -> Mesh {
    Mesh {
        vertices: vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [0., 0., 10.],
            [10., 0., 10.],
            [10., 10., 10.],
            [0., 10., 10.],
        ],
        triangles: vec![
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
        ..Mesh::default()
    }
}

fn ring() -> Mesh {
    let mut mesh = Mesh::default();
    let (major, minor) = (32, 16);
    for a in 0..major {
        let (s, c) = (std::f64::consts::TAU * a as f64 / major as f64).sin_cos();
        for b in 0..minor {
            let (z, r) = (std::f64::consts::TAU * b as f64 / minor as f64).sin_cos();
            mesh.vertices
                .push([(8. + 2. * r) * c, (8. + 2. * r) * s, 2. * z]);
            let next_a = (a + 1) % major;
            let next_b = (b + 1) % minor;
            let [p, q, r, t] = [
                a * minor + b,
                next_a * minor + b,
                next_a * minor + next_b,
                a * minor + next_b,
            ];
            mesh.triangles.extend([[p, q, r], [p, r, t]]);
        }
    }
    mesh
}

fn closed_volume(mesh: &Mesh) -> f64 {
    let mut edges = BTreeMap::new();
    let mut volume = 0.;
    for &[a, b, c] in &mesh.triangles {
        let [p, q, r] = [a, b, c].map(|i| DVec3::from(mesh.vertices[i as usize]));
        assert!((q - p).cross(r - p).length() > 1e-12);
        volume += p.dot(q.cross(r)) / 6.;
        for (i, j) in [(a, b), (b, c), (c, a)] {
            let v = edges.entry((i.min(j), i.max(j))).or_insert((0, 0));
            if i < j {
                v.0 += 1;
            } else {
                v.1 += 1;
            }
        }
    }
    assert!(
        edges.values().all(|v| *v == (1, 1)),
        "An edge is open or has inconsistent winding."
    );
    volume
}

#[test]
fn cube_cut_at_four() {
    let parts = cut(&cube(), [0., 0., 4.], UP, Keep::Below, true).unwrap();
    assert!((closed_volume(&parts[0]) - 400.).abs() < 1e-9);
}

#[test]
fn both_sides_sum_to_the_whole() {
    let parts = cut(&cube(), [0., 0., 4.], UP, Keep::Both, true).unwrap();
    assert_eq!(parts.len(), 2);
    let mut volumes = parts.iter().map(closed_volume).collect::<Vec<_>>();
    volumes.sort_by(f64::total_cmp);
    assert!((volumes[0] - 400.).abs() < 1e-9 && (volumes[1] - 600.).abs() < 1e-9);
}

#[test]
fn torus_cut_gives_two_loops() {
    let mesh = ring();
    let loops = slice(&mesh, [0.; 3], UP);
    assert_eq!(loops.len(), 2);
    assert!(
        loops
            .iter()
            .all(|path| path.first() == path.last() && path.len() > 3)
    );
    let part = cut(&mesh, [0.; 3], UP, Keep::Above, true)
        .unwrap()
        .remove(0);
    assert!((closed_volume(&part) * 2. - closed_volume(&mesh)).abs() < 1e-8);
    for tri in &part.triangles {
        let [a, b, c] = tri.map(|i| DVec3::from(part.vertices[i as usize]));
        if [a, b, c].iter().all(|p| p.z.abs() < 1e-8) {
            assert!(
                ((a + b + c) / 3.).length() > 5.9,
                "A cap triangle crosses the hole."
            );
        }
    }
}

#[test]
fn plane_through_vertices() {
    let mesh = cube();
    let top = cut(&mesh, [0., 0., 10.], UP, Keep::Below, true)
        .unwrap()
        .remove(0);
    assert!((closed_volume(&top) - 1000.).abs() < 1e-9);
    let diagonal = cut(&mesh, [0., 0., 10.], [1., 0., 1.], Keep::Above, true)
        .unwrap()
        .remove(0);
    assert!((closed_volume(&diagonal) - 500.).abs() < 1e-9);
}

#[test]
fn plane_outside_the_mesh() {
    let mesh = cube();
    let origin = [0., 0., 15.];
    let part = cut(&mesh, origin, UP, Keep::Below, true).unwrap().remove(0);
    assert_eq!(part.triangles, mesh.triangles);
    assert_eq!(
        cut(&mesh, origin, UP, Keep::Above, true).unwrap_err(),
        "The plane does not cut the mesh. The selected side is empty."
    );
}

#[test]
fn open_cut_does_not_add_cap_faces() {
    let part = cut(&cube(), [0., 0., 3.], UP, Keep::Below, false)
        .unwrap()
        .remove(0);
    assert!(!part.triangles.iter().any(|t| {
        t.iter()
            .all(|&i| (part.vertices[i as usize][2] - 3.).abs() < 1e-12)
    }));
    assert!(cut(&cube(), [0.; 3], [0.; 3], Keep::Below, true).is_err());
}
