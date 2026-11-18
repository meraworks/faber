mod common;
use common::{append, cube, signed, sphere};
use faber_meshtools::{RepairOptions, repair, try_repair};
#[test]
fn missing_face_is_filled() {
    let mut m = cube(10., 1);
    m.triangles.truncate(10);
    let (fixed, l) = repair(&m, &RepairOptions::default());
    assert!(l.left.watertight && l.filled_holes == 1);
    assert!((signed(&fixed) - 1000.).abs() < 1e-9);
}
#[test]
fn flipped_triangle_is_turned() {
    let mut m = cube(10., 1);
    m.triangles[4].swap(0, 1);
    let (_, l) = repair(&m, &RepairOptions::default());
    assert_eq!(l.flipped, 1);
    assert!(l.left.problems.is_empty());
}
#[test]
fn inside_out_cube_is_turned() {
    let mut m = cube(10., 1);
    for t in &mut m.triangles {
        t.swap(0, 1);
    }
    let (f, l) = repair(&m, &RepairOptions::default());
    assert_eq!(l.flipped, 12);
    assert!((signed(&f) - 1000.).abs() < 1e-9);
}
#[test]
fn saddle_hole_is_filled_outward() {
    let mut m = sphere(10., 64, 48);
    m.triangles.retain(|t| {
        !t.iter().all(|&i| {
            let [x, y, z] = m.vertices[i as usize];
            z > 8.9 + 0.35 * (3. * y.atan2(x)).sin()
        })
    });
    let (f, l) = repair(&m, &RepairOptions::default());
    assert_eq!(l.filled_holes, 1);
    assert!(l.left.watertight && l.left.flipped == 0);
    assert!((signed(&f) / (4. / 3. * std::f64::consts::PI * 1000.) - 1.).abs() < 0.02);
}
#[test]
fn dust_shell_is_removed() {
    let mut m = cube(10., 1);
    append(&mut m, &cube(0.1, 1), [20., 0., 0.]);
    let (_, l) = repair(
        &m,
        &RepairOptions {
            drop_shells_below_mm3: Some(1.),
            ..Default::default()
        },
    );
    assert_eq!((l.removed_shells, l.left.shells), (1, 1));
}
#[test]
fn non_manifold_edge_stays_reported() {
    let mut m = cube(10., 1);
    append(&mut m, &cube(10., 1), [10., 10., 0.]);
    let (_, l) = repair(&m, &RepairOptions::default());
    assert_eq!(l.left.non_manifold_edges, 1);
}
#[test]
fn repeated_triangles_and_zero_area_are_removed() {
    let mut m = cube(10., 1);
    let mut t = m.triangles[0];
    t.swap(1, 2);
    m.triangles.extend([t, [0, 0, 0]]);
    let (_, l) = repair(&m, &Default::default());
    assert_eq!((l.removed_duplicate, l.removed_degenerate), (1, 1));
    assert!(l.left.watertight);
}
#[test]
fn negative_weld_is_refused() {
    assert!(
        try_repair(
            &cube(1., 1),
            &RepairOptions {
                weld_mm: -1.,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn a_closed_inner_void_stays_empty() {
    let mut m = cube(10., 1);
    let mut cavity = cube(8., 1);
    for t in &mut cavity.triangles {
        t.swap(1, 2);
    }
    append(&mut m, &cavity, [1., 1., 1.]);
    let (f, l) = repair(&m, &Default::default());
    assert_eq!(l.flipped, 0);
    assert!((signed(&f) - 488.).abs() < 1e-8);
}

#[test]
fn hollow_rim_is_closed_with_an_annulus() {
    let mut outer = cube(10., 1);
    outer.triangles.truncate(10);
    let mut inner = cube(8., 1);
    inner.triangles.truncate(10);
    for t in &mut inner.triangles {
        t.swap(0, 1);
    }
    append(&mut outer, &inner, [1., 1., 2.]);
    let (_, l) = repair(&outer, &Default::default());
    assert_eq!(l.left.shells, 1);
    assert!(l.left.watertight);
    assert!((l.left.area_mm2 - 856.).abs() < 1e-9);
    assert!((l.left.volume_mm3.unwrap() - 488.).abs() < 1e-9);
}
