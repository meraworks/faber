use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn geometric_edge_selectors_follow_parameter_changes_and_refuse_wrong_counts() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Edges".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let recipe = json!({"name":"Rounded block","colour":[100,140,170],"geometry":{"op":"history","output":"round","features":[
        {"id":"block","geometry":{"op":"box","size":[10,20,"height"]}},
        {"id":"round","geometry":{"op":"fillet","source":"block","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":1}}
    ]}});
    let part: Part<Scalar> = serde_json::from_value(recipe.clone()).unwrap();
    project.put_part("part".into(), part).unwrap();
    let area = 200. - (4. - std::f64::consts::PI);
    assert!((project.bodies[0].volume - area * 30.).abs() < 1e-5);
    project
        .set_params([("height".into(), "60".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - area * 60.).abs() < 1e-5);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let mut wrong = recipe;
    wrong["geometry"]["features"][1]["geometry"]["edges"]["expect"] = json!(1);
    let wrong: Part<Scalar> = serde_json::from_value(wrong).unwrap();
    let error = project.put_part("part".into(), wrong).unwrap_err();
    assert!(error.contains("expected 1"), "{error}");
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(directory.path()).unwrap()
    );
    let too_large = serde_json::from_value(json!({"op":"fillet","source":"block","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":100})).unwrap();
    let error = project
        .edit_feature("part", "round", too_large)
        .unwrap_err();
    assert!(error.contains("tested value"), "{error}");
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(directory.path()).unwrap()
    );
    let chamfer = serde_json::from_value(json!({"op":"chamfer","source":"block","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"distance":1})).unwrap();
    project.edit_feature("part", "round", chamfer).unwrap();
    assert!((project.bodies[0].volume - 11880.).abs() < 1e-5);
}

#[test]
fn shell_face_selection_follows_height_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Shell".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Open box","colour":[100,140,170],"geometry":{"op":"history","output":"walls","features":[
        {"id":"block","geometry":{"op":"box","size":[10,20,"height"]}},
        {"id":"walls","geometry":{"op":"shell","source":"block","faces":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"thickness":1}}
    ]}})).unwrap();
    project.put_part("box".into(), part).unwrap();
    assert!((project.bodies[0].volume - 1824.).abs() < 1e-5);
    project
        .set_params([("height".into(), "60".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 3504.).abs() < 1e-5);
}
