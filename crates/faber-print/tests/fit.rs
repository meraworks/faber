use faber_model::Clearance;
use faber_ports::{CylinderFace, FaceInfo};
use faber_print::{FitClass, StackItem, cylinder_fits, stack};
fn clear() -> Clearance {
    Clearance {
        press: 0.,
        snug: 0.15,
        slide: 0.25,
        loose: 0.4,
    }
}
fn face(d: f64, hole: bool, offset: f64) -> FaceInfo {
    FaceInfo {
        index: if hole { 3 } else { 7 },
        created_by: None,
        area: std::f64::consts::PI * d * 10.,
        centre: [offset, 0., 5.],
        normal: None,
        cylinder: Some(CylinderFace {
            start: [offset, 0., 0.],
            axis: [0., 0., 1.],
            length: 10.,
            radius: d / 2.,
            internal: hole,
        }),
    }
}
#[test]
fn pin_in_hole_is_a_slide_fit() {
    let f = cylinder_fits(&[face(5.25, true, 0.)], &[face(5., false, 0.)], &clear());
    assert_eq!(f[0].class, FitClass::Slide);
    assert_eq!(f[0].clearance_mm, 0.25);
    assert_eq!((f[0].a_face, f[0].b_face), (3, 7));
}
#[test]
fn equal_radius_is_a_zero_clearance_fit() {
    let f = cylinder_fits(&[face(5., false, 0.)], &[face(5., true, 0.)], &clear());
    assert_eq!(f[0].class, FitClass::Press);
    assert_eq!(f[0].clearance_mm, 0.);
}
#[test]
fn larger_shaft_is_interference() {
    let f = cylinder_fits(&[face(5., true, 0.)], &[face(5.1, false, 0.)], &clear());
    assert_eq!(f[0].class, FitClass::Interference);
}
#[test]
fn offset_axes_are_not_a_fit() {
    assert!(cylinder_fits(&[face(5., true, 0.)], &[face(5., false, 0.5)], &clear()).is_empty());
}

#[test]
fn only_overlapping_holes_and_shafts_are_matched() {
    let hole = face(5.25, true, 0.);
    let mut shaft = face(5., false, 0.);
    assert!(
        cylinder_fits(
            std::slice::from_ref(&shaft),
            std::slice::from_ref(&shaft),
            &clear()
        )
        .is_empty()
    );
    shaft.cylinder.as_mut().unwrap().start[2] = 10.;
    assert!(
        cylinder_fits(
            std::slice::from_ref(&hole),
            std::slice::from_ref(&shaft),
            &clear()
        )
        .is_empty()
    );
    shaft.cylinder.as_mut().unwrap().axis[2] = -1.;
    let fits = cylinder_fits(&[hole], &[shaft], &clear());
    assert_eq!(fits[0].axis_point, [0., 0., 5.]);
}
#[test]
fn stack_of_three() {
    let items = [(10., 1), (20., 1), (5., -1)].map(|(nominal, sign)| StackItem {
        nominal,
        plus: 0.1,
        minus: 0.1,
        sign,
    });
    let s = stack(&items);
    assert_eq!(s.nominal, 25.);
    for i in 0..2 {
        assert!((s.worst[i] - [24.7, 25.3][i]).abs() < 1e-9);
        assert!((s.rss[i] - [24.8268, 25.1732][i]).abs() < 1e-4);
    }
}
#[test]
fn subtracting_an_asymmetric_item_reverses_its_tolerance() {
    let s = stack(&[StackItem {
        nominal: 10.,
        sign: -1,
        plus: 1.,
        minus: 2.,
    }]);
    assert_eq!(s.worst, [-11., -8.]);
    assert_eq!(s.rss, s.worst);
}
