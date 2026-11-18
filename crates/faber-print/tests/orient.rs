mod common;
use common::{block, cells, cylinder, printer};
use faber_print::{Weights, rank};

#[test]
fn l_bracket_best_side_has_no_support() {
    let m = cells([&[0., 2., 12.], &[0., 4.], &[0., 2., 12.]], |[x, _, z]| {
        x == 0 || z == 1
    });
    let choices = rank(&m, &printer(), Weights::default()).unwrap();
    assert_eq!(choices[0].support_mm3, 0.);
    assert!(choices.len() <= 5);
}

#[test]
fn t_part_does_not_stand_on_its_stem() {
    let m = cells(
        [&[0., 8., 12., 20.], &[0., 10.], &[0., 10., 12.]],
        |[x, _, z]| z == 1 || x == 1,
    );
    let best = &rank(&m, &printer(), Weights::default()).unwrap()[0];
    assert!(best.down[2] > -0.99);
    assert_eq!(best.support_mm3, 0.);
}

#[test]
fn cylinder_stands_on_its_end() {
    let m = cylinder(10., 5.);
    let best = &rank(&m, &printer(), Weights::default()).unwrap()[0];
    assert!(best.down[2].abs() > 0.999);
    assert!((best.height_mm - 5.).abs() < 1e-8);
}

#[test]
fn too_large_part_is_reported() {
    let c = rank(&block([300.; 3]), &printer(), Weights::default()).unwrap();
    assert!(
        !c.is_empty()
            && c.iter()
                .all(|v| !v.fits && v.size_mm.iter().any(|&s| s > 256.))
    );
}

#[test]
fn long_bar_fits_across_the_diagonal() {
    let m = block([300., 10., 10.]);
    let c = rank(&m, &printer(), Weights::default()).unwrap();
    assert!(c[0].fits);
    assert!(c[0].size_mm.iter().all(|&s| s <= 256.00001));
    let q = glam::DQuat::from_euler(
        glam::EulerRot::ZYX,
        c[0].rotation_deg[2].to_radians(),
        c[0].rotation_deg[1].to_radians(),
        c[0].rotation_deg[0].to_radians(),
    );
    let v: Vec<_> = m
        .vertices
        .iter()
        .map(|&p| q * glam::DVec3::from(p))
        .collect();
    let lo = v.iter().copied().reduce(glam::DVec3::min).unwrap();
    let hi = v.iter().copied().reduce(glam::DVec3::max).unwrap();
    assert!((hi - lo - glam::DVec3::from(c[0].size_mm)).length() < 1e-6);
}

#[test]
fn a_close_diagonal_fit_uses_the_actual_bed_size() {
    let mut p = printer();
    let (s, c) = 25.321_f64.to_radians().sin_cos();
    p.bed_mm = [350. * c + 5. * s + 1e-7, 350. * s + 5. * c + 1e-7, 6.];
    let choices = rank(&block([350., 5., 5.]), &p, Weights::default()).unwrap();
    assert!(choices[0].fits);
    assert!((choices[0].size_mm[0] - p.bed_mm[0]).abs() < 1e-5);
    assert!((choices[0].size_mm[1] - p.bed_mm[1]).abs() < 1e-5);
}
