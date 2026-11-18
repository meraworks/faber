mod common;
use common::{block, cells, printer};
use faber_print::{Kind, check, check_with_walls};
use glam::DVec3;
const UP: [f64; 3] = [0., 0., 1.];

#[test]
fn shelled_box_wall_is_measured() {
    let side = [0., 1.2, 18.8, 20.];
    let m = cells([&side; 3], |p| p != [1; 3]);
    let mut p = printer();
    p.min_wall_mm = 1.6;
    let (r, walls) = check_with_walls(&[&m], &p, UP).unwrap();
    assert!((r.min_wall_mm - 1.2).abs() < 0.05);
    // Six exterior squares and the six faces of one connected inner shell.
    let face_area = 17.6_f64.powi(2);
    assert!((r.thin_wall_mm2 - 12. * face_area).abs() < 1e-6);
    assert_eq!(
        r.findings
            .iter()
            .filter(|f| f.kind == Kind::ThinWall)
            .count(),
        7
    );
    assert_eq!(walls[0].len(), m.triangles.len());
}

#[test]
fn solid_box_has_no_thin_wall() {
    let r = check(&[&block([20.; 3])], &printer(), UP).unwrap();
    assert_eq!(r.thin_wall_mm2, 0.);
    assert_eq!(r.bed_contact_mm2, 400.);
    assert_eq!(r.layers, 100);
}

#[test]
fn a_wall_at_the_nominal_limit_is_not_thin() {
    let mut p = printer();
    p.min_wall_mm = 1.4;
    let m = block([1.4, 8., 10.]);
    let r = check(&[&m], &p, UP).unwrap();
    assert_eq!(r.thin_wall_mm2, 0.);
}

#[test]
fn unused_vertices_do_not_change_the_size_or_islands() {
    let mut m = block([8.; 3]);
    m.vertices.push([500., 50., 12.]);
    let r = check(&[&m], &printer(), UP).unwrap();
    assert_eq!(r.size_mm, [8.; 3]);
    assert_eq!(r.islands, 0);
}

#[test]
fn table_support_volume() {
    let xy = [0., 8., 12., 20.];
    let m = cells([&xy, &xy, &[0., 10., 12.]], |[x, y, z]| {
        z == 1 || x == 1 && y == 1
    });
    let r = check(&[&m], &printer(), UP).unwrap();
    assert!((r.support_mm3 - 3840.).abs() < 192., "{}", r.support_mm3);
    assert!((r.overhang_mm2 - 384.).abs() < 1e-6);
}

#[test]
fn l_bracket_flat_needs_no_support() {
    let m = cells([&[0., 2., 12.], &[0., 4.], &[0., 2., 12.]], |[x, _, z]| {
        x == 0 || z == 0
    });
    assert_eq!(check(&[&m], &printer(), UP).unwrap().support_mm3, 0.);
}

#[test]
fn a_pocket_floor_has_material_below_it() {
    let xy = [0., 2., 12., 14.];
    let z = [0., 2., 10.];
    let tray = cells([&xy, &xy, &z], |[x, y, z]| z == 0 || x != 1 || y != 1);
    let report = check(&[&tray], &printer(), UP).unwrap();
    assert_eq!(report.islands, 0);
}

#[test]
fn hanging_tooth_is_an_island() {
    let xs = [0., 2., 5., 7., 10., 12.];
    let zs = [0., 4., 8., 10.];
    for tooth in [false, true] {
        let m = cells([&xs, &[0., 2.], &zs], |[x, _, z]| {
            x == 0 || x == 4 || z == 2 || tooth && x == 2 && z == 1
        });
        assert_eq!(
            check(&[&m], &printer(), UP).unwrap().islands,
            u32::from(tooth)
        );
    }
}

#[test]
fn long_bridge_needs_two_support_edges() {
    let xs = [0., 2., 22., 24.];
    let zs = [0., 8., 10.];
    for has_second_pillar in [false, true] {
        let m = cells([&xs, &[0., 4.], &zs], |[x, _, z]| {
            z == 1 || x == 0 || has_second_pillar && x == 2
        });
        let r = check(&[&m], &printer(), UP).unwrap();
        let f = r.findings.iter().find(|f| f.kind == Kind::LongBridge);
        if has_second_pillar {
            assert!((f.unwrap().value - 20.).abs() < 1e-6);
        } else {
            assert!(f.is_none());
        }
    }
}

#[test]
fn tall_thin_tower_reports_stability() {
    let r = check(&[&block([2., 2., 20.])], &printer(), UP).unwrap();
    let get = |kind| r.findings.iter().find(|f| f.kind == kind).unwrap().value;
    assert_eq!(get(Kind::SmallContact), 4.);
    assert_eq!(get(Kind::TallAndNarrow), 10.);
}

#[test]
fn too_large_part() {
    let r = check(&[&block([300., 5., 5.])], &printer(), UP).unwrap();
    assert!(!r.fits);
    assert!(
        r.findings
            .iter()
            .any(|f| f.message == "Size 300 mm along X is larger than the bed 256 mm.")
    );
}

#[test]
fn open_mesh_is_refused() {
    let mut m = block([2.; 3]);
    m.triangles.pop();
    assert_eq!(
        check(&[&m], &printer(), UP).unwrap_err(),
        "The mesh is open (3 open edges). Repair it first."
    );
}

#[test]
fn check_uses_the_placement() {
    let mut m = block([30., 2., 4.]);
    let place = glam::DMat3::from_rotation_y(std::f64::consts::FRAC_PI_2);
    for v in &mut m.vertices {
        *v = (place * DVec3::from(*v)).into()
    }
    let r = check(&[&m], &printer(), UP).unwrap();
    assert!((r.height_mm - 30.).abs() < 1e-8);
}

#[test]
fn stacked_parts_share_one_bed_and_stop_support_at_the_lower_part() {
    let lower = block([20., 20., 5.]);
    let mut upper = block([4., 4., 2.]);
    for p in &mut upper.vertices {
        p[2] += 7.;
    }
    let r = check(&[&lower, &upper], &printer(), UP).unwrap();
    assert_eq!(r.bed_contact_mm2, 400.);
    assert_eq!(r.overhang_mm2, 16.);
    assert!((r.support_mm3 - 32.).abs() < 1e-6);
    assert_eq!(r.islands, 1);
}
