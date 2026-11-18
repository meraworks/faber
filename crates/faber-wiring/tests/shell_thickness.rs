use faber_model::{Part, Scalar};
use serde_json::{Value, json};

const SIGN_RULE: &str =
    "Shell thickness must be positive. The shell grows inward from the outer faces.";

/// Box 100 x 60 x 40 mm. The top face is removed. The walls use `thickness`.
fn box_shell(thickness: Value, overrides: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Tray","colour":[100,140,170],"geometry":{"op":"history","output":"walls","features":[
        {"id":"block","geometry":{"op":"box","size":[100,60,40]}},
        {"id":"walls","geometry":{"op":"shell","source":"block",
            "faces":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},
            "thickness":thickness,"thickness_overrides":overrides}}
    ]}}))
    .unwrap()
}

fn floor_override(thickness: f64) -> Value {
    json!([{"faces":{"normal":[0,0,-1],"expect":1,"tolerance":0.000001},"thickness":thickness}])
}

#[test]
fn floor_override_gives_the_bottom_its_own_thickness() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    project
        .put_part("tray".into(), box_shell(json!(2), floor_override(4.)))
        .unwrap();
    // Outer 100 x 60 x 40. Cavity (100 - 4) x (60 - 4) x (40 - 4) = 193536.
    assert!((project.bodies[0].volume - 46464.).abs() < 1e-5);
}

#[test]
fn override_that_matches_no_face_is_refused_and_the_file_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    project
        .put_part("tray".into(), box_shell(json!(2), floor_override(4.)))
        .unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let nothing = json!([{"faces":{"in_box":[[500,500,500],[600,600,600]],"expect":1,"tolerance":0.000001},"thickness":4}]);
    let error = project
        .put_part("tray".into(), box_shell(json!(2), nothing))
        .unwrap_err();
    assert!(error.contains("found 0, expected 1"), "{error}");
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(directory.path()).unwrap()
    );
}

#[test]
fn override_on_a_removed_face_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    let top = json!([{"faces":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"thickness":4}]);
    let error = project
        .put_part("tray".into(), box_shell(json!(2), top))
        .unwrap_err();
    assert!(error.contains("more than one shell rule"), "{error}");
}

#[test]
fn two_overrides_on_one_face_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    let twice = json!([floor_override(4.)[0], floor_override(3.)[0]]);
    let error = project
        .put_part("tray".into(), box_shell(json!(2), twice))
        .unwrap_err();
    assert!(error.contains("more than one shell rule"), "{error}");
}

#[test]
fn non_positive_thickness_is_refused_with_the_sign_rule() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    for thickness in [json!(-2), json!(0)] {
        let error = project
            .put_part("tray".into(), box_shell(thickness, json!([])))
            .unwrap_err();
        assert!(error.contains(SIGN_RULE), "{error}");
    }
    let error = project
        .put_part("tray".into(), box_shell(json!(2), floor_override(-4.)))
        .unwrap_err();
    assert!(error.contains(SIGN_RULE), "{error}");
}

#[test]
fn shell_kernel_failure_names_the_feature_and_gives_the_hint() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell thickness".into())).unwrap();
    // Fillet radius 2 mm, shell thickness 3 mm: the inner offset of each corner has no radius.
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Rounded tray","colour":[100,140,170],"geometry":{"op":"history","output":"walls","features":[
        {"id":"block","geometry":{"op":"box","size":[100,60,40]}},
        {"id":"round","geometry":{"op":"fillet","source":"block","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":2}},
        {"id":"walls","geometry":{"op":"shell","source":"round",
            "faces":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"thickness":3}}
    ]}}))
    .unwrap();
    let error = project.put_part("tray".into(), part).unwrap_err();
    assert!(error.contains("Feature walls (shell) failed"), "{error}");
    assert!(error.contains("Shell before you fillet or chamfer the edges that touch the removed faces. A fillet radius smaller than the thickness can also fail."), "{error}");
}
