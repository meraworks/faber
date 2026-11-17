use faber_model::{Part, Scalar};
use serde_json::json;

fn block_part(output: &str, feature: serde_json::Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"history","output":output,"features":[
        {"id":"block","geometry":{"op":"box","size":[10,20,30]}},
        feature
    ]}}))
    .unwrap()
}

// Four 1 mm chamfers on the top edges of 10 x 20 x 30 remove
// (10 + 20) * 1^2 - 4/3 * 1^3 mm3 in total, corners included.
fn volume_after_top_chamfer() -> f64 {
    6000. - (30. - 4. / 3.)
}

#[test]
fn edge_count_mismatch_lists_the_matched_edges() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Help".into())).unwrap();
    let part = block_part(
        "round",
        json!({"id":"round","geometry":{"op":"chamfer","source":"block",
        "edges":{"parallel":[0,0,1],"expect":1,"tolerance":0.000001},"distance":1}}),
    );
    let error = project.put_part("part".into(), part).unwrap_err();
    assert!(
        error.contains("Edge selector found 4, expected 1"),
        "{error}"
    );
    assert!(error.contains("Matched items"), "{error}");
    assert_eq!(error.matches("\n  edge ").count(), 4, "{error}");
    assert!(!error.contains("Closest rejected"), "{error}");
}

#[test]
fn edge_selector_with_no_match_lists_three_closest_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Help".into())).unwrap();
    let part = block_part(
        "round",
        json!({"id":"round","geometry":{"op":"chamfer","source":"block",
        "edges":{"parallel":[1,0,0],"length":99,"expect":1,"tolerance":0.000001},"distance":1}}),
    );
    let error = project.put_part("part".into(), part).unwrap_err();
    assert!(
        error.contains("Edge selector found 0, expected 1"),
        "{error}"
    );
    assert!(error.contains("Closest rejected items"), "{error}");
    assert_eq!(error.matches("\n  edge ").count(), 3, "{error}");
    assert_eq!(error.matches("rejected by length").count(), 3, "{error}");
    assert!(error.contains("length 10.000"), "{error}");
}

#[test]
fn in_box_selects_the_top_edges_and_chamfers_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Box".into())).unwrap();
    let part = block_part(
        "round",
        json!({"id":"round","geometry":{"op":"chamfer","source":"block",
        "edges":{"in_box":[[-0.5,-0.5,29.5],[10.5,20.5,30.5]],"expect":4,"tolerance":0.000001},"distance":1}}),
    );
    project.put_part("part".into(), part).unwrap();
    assert!((project.bodies[0].volume - volume_after_top_chamfer()).abs() < 1e-5);
}

#[test]
fn of_face_selects_the_perimeter_of_the_top_face() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Box".into())).unwrap();
    let part = block_part(
        "round",
        json!({"id":"round","geometry":{"op":"chamfer","source":"block",
        "edges":{"of_face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"expect":4,"tolerance":0.000001},"distance":1}}),
    );
    project.put_part("part".into(), part).unwrap();
    assert!((project.bodies[0].volume - volume_after_top_chamfer()).abs() < 1e-5);
}

#[test]
fn face_in_box_selects_the_top_face_only() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Shell".into())).unwrap();
    let part = block_part(
        "walls",
        json!({"id":"walls","geometry":{"op":"shell","source":"block",
        "faces":{"in_box":[[-1,-1,25],[11,21,31]],"expect":1,"tolerance":0.000001},"thickness":1}}),
    );
    project.put_part("part".into(), part).unwrap();
    assert!((project.bodies[0].volume - 1824.).abs() < 1e-5);
}

#[test]
fn face_count_mismatch_lists_five_matched_faces() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Shell".into())).unwrap();
    let part = block_part(
        "walls",
        json!({"id":"walls","geometry":{"op":"shell","source":"block",
        "faces":{"in_box":[[-1,-1,-1],[11,21,31]],"expect":1,"tolerance":0.000001},"thickness":1}}),
    );
    let error = project.put_part("part".into(), part).unwrap_err();
    assert!(
        error.contains("Face selector found 6, expected 1"),
        "{error}"
    );
    assert_eq!(error.matches("\n  face ").count(), 5, "{error}");
    assert!(error.contains(" normal ["), "{error}");
    assert!(error.contains(" area "), "{error}");
}
