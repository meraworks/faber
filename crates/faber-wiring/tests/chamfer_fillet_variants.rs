use faber_model::{Part, Scalar};
use serde_json::{Value, json};
use std::f64::consts::PI;

/// Solid cube, 20 mm sides. The box starts at its origin corner.
const CUBE: f64 = 8000.;

/// The vertical edge at x 20, y 20. Its centre is (20, 20, 10).
fn edge() -> Value {
    json!({"parallel":[0,0,1],"near_centre":[20,20,10],"tolerance":0.000001,"expect":1})
}

/// Merge `extra` keys into the feature object `base`.
fn with(mut base: Value, extra: Value) -> Value {
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn chamfer(extra: Value) -> Value {
    with(
        json!({"op":"chamfer","source":"block","edges":edge(),"distance":2}),
        extra,
    )
}

fn fillet(extra: Value) -> Value {
    with(
        json!({"op":"fillet","source":"block","edges":edge(),"radius":1}),
        extra,
    )
}

fn cube(feature: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Cube","colour":[100,140,170],"geometry":{"op":"history","output":"edit","features":[
        {"id":"block","geometry":{"op":"box","size":[20,20,20]}},
        {"id":"edit","geometry":feature}
    ]}}))
    .unwrap()
}

fn volume_of(feature: Value) -> f64 {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    project.put_part("cube".into(), cube(feature)).unwrap();
    project.bodies[0].volume
}

#[test]
fn two_distance_chamfer_removes_the_prism() {
    // Prism of 0.5 * 2 * 4 * 20 = 80 mm3.
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    project
        .put_part("cube".into(), cube(chamfer(json!({"distance2":4}))))
        .unwrap();
    assert!((project.bodies[0].volume - (CUBE - 80.)).abs() < 1e-4);
    let (_, faces) = project.feature_topology("cube", "edit").unwrap();
    assert_eq!(faces.len(), 7);
    let new = faces
        .iter()
        .filter(|face| face.created_by.as_deref() == Some(&["edit".into()]))
        .count();
    assert_eq!(new, 1);
}

#[test]
fn distance_and_angle_chamfer_equals_the_equal_distance_chamfer() {
    // Distance 3 at 45 degrees gives two equal sides of 3. Removes 0.5 * 3 * 3 * 20 = 90 mm3.
    let equal = volume_of(chamfer(json!({"distance":3})));
    let angled = volume_of(chamfer(json!({"distance":3,"angle_deg":45})));
    assert!((equal - (CUBE - 90.)).abs() < 1e-4);
    assert!((angled - equal).abs() < 1e-4);
}

#[test]
fn chamfer_face_option_accepts_a_touching_face() {
    let face = json!({"normal":[1,0,0],"expect":1,"tolerance":0.000001});
    let volume = volume_of(chamfer(json!({"distance2":4,"face":face})));
    assert!((volume - (CUBE - 80.)).abs() < 1e-4);
}

#[test]
fn chamfer_face_option_refuses_a_face_that_does_not_touch_the_edge() {
    let face = json!({"normal":[0,0,1],"expect":1,"tolerance":0.000001});
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    let error = project
        .put_part(
            "cube".into(),
            cube(chamfer(json!({"distance2":4,"face":face}))),
        )
        .unwrap_err();
    assert!(error.contains("does not touch"), "{error}");
}

#[test]
fn variable_fillet_removes_between_the_constant_radii() {
    // A constant fillet of radius r removes (1 - pi / 4) * r^2 * 20 mm3.
    let one = (1. - PI / 4.) * 1. * 20.;
    let three = (1. - PI / 4.) * 9. * 20.;
    let variable = CUBE - volume_of(fillet(json!({"radius_end":3})));
    assert!(variable > one, "{variable} <= {one}");
    assert!(variable < three, "{variable} >= {three}");
    assert!((CUBE - volume_of(fillet(json!({}))) - one).abs() < 1e-3);
}

#[test]
fn invalid_values_are_refused_and_the_file_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    project
        .put_part("cube".into(), cube(chamfer(json!({"distance2":4}))))
        .unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let cases = [
        (
            chamfer(json!({"distance2":-1})),
            "distance2 must be positive",
        ),
        (
            chamfer(json!({"distance2":0})),
            "distance2 must be positive",
        ),
        (
            chamfer(json!({"angle_deg":0})),
            "angle_deg must be positive",
        ),
        (chamfer(json!({"angle_deg":90})), "must be less than 90"),
        (chamfer(json!({"distance2":4,"angle_deg":45})), "not both"),
        (
            fillet(json!({"radius_end":0})),
            "radius_end must be positive",
        ),
        (
            fillet(json!({"radius_end":-2})),
            "radius_end must be positive",
        ),
    ];
    for (feature, message) in cases {
        let error = project.put_part("cube".into(), cube(feature)).unwrap_err();
        assert!(error.contains(message), "{error}");
        assert_eq!(
            before,
            faber_wiring::project_fingerprint(directory.path()).unwrap()
        );
    }
}

#[test]
fn parameters_rebuild_the_distances() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    project
        .set_params([("a".into(), "2".into()), ("b".into(), "4".into())].into())
        .unwrap();
    project
        .put_part(
            "cube".into(),
            cube(chamfer(json!({"distance":"a","distance2":"b"}))),
        )
        .unwrap();
    assert!((project.bodies[0].volume - (CUBE - 80.)).abs() < 1e-4);
    project
        .set_params([("b".into(), "6".into())].into())
        .unwrap();
    // 0.5 * 2 * 6 * 20 = 120 mm3.
    assert!((project.bodies[0].volume - (CUBE - 120.)).abs() < 1e-4);
}

#[test]
fn reopened_project_keeps_the_variant() {
    let directory = tempfile::tempdir().unwrap();
    {
        let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
        project
            .put_part("cube".into(), cube(chamfer(json!({"distance2":4}))))
            .unwrap();
    }
    let reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert!((reopened.bodies[0].volume - (CUBE - 80.)).abs() < 1e-4);
    let text = std::fs::read_to_string(directory.path().join("parts/cube.json")).unwrap();
    assert!(text.contains("distance2"));
}

#[test]
fn constant_chamfer_file_has_no_new_keys() {
    // Old files keep their form. The new keys are written only when set.
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cube".into())).unwrap();
    project
        .put_part("cube".into(), cube(chamfer(json!({}))))
        .unwrap();
    assert!((project.bodies[0].volume - (CUBE - 40.)).abs() < 1e-4);
    let text = std::fs::read_to_string(directory.path().join("parts/cube.json")).unwrap();
    assert!(!text.contains("distance2"));
    assert!(!text.contains("angle_deg"));
    assert!(!text.contains("radius_end"));
}
