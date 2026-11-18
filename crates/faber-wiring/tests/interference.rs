use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn strict_batch_refuses_new_interference_without_a_saved_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Strict".into())).unwrap();
    let operations = serde_json::from_value(json!([
        {"tool":"put_part","id":"a","part":{"name":"A","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]}}},
        {"tool":"put_part","id":"b","part":{"name":"B","colour":[100,140,170],"geometry":{"op":"box","size":[2,2,2]},"placement":{"translation":[3,3,3]}}}
    ])).unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    assert!(project.batch_strict(operations).is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    assert!(project.bodies.is_empty());
    assert!(project.undo().is_err());
}

#[test]
fn checked_export_refuses_interference_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Export".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(
        json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]}}),
    )
    .unwrap();
    project.put_part("a".into(), part.clone()).unwrap();
    project.put_part("b".into(), part).unwrap();
    let output = dir.path().join("out");
    assert!(
        faber_wiring::export_checked(
            &project.bodies,
            &project.data.contacts,
            &output,
            "stl",
            false
        )
        .is_err()
    );
    assert!(!output.exists());
    assert_eq!(
        faber_wiring::export_checked(
            &project.bodies,
            &project.data.contacts,
            &output,
            "stl",
            true
        )
        .unwrap()
        .len(),
        2
    );
}

#[test]
fn project_interference_uses_placed_bodies_without_changing_the_design() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Interference".into())).unwrap();
    let part = |size: f64, offset: f64| -> Part<Scalar> {
        serde_json::from_value(json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[size,size,size]},"placement":{"translation":[offset,offset,offset]}})).unwrap()
    };
    project.put_part("outer".into(), part(10., 0.)).unwrap();
    project.put_part("inner".into(), part(2., 3.)).unwrap();
    let before = project.data.clone();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert_eq!(report.violations.len(), 1);
    assert!(report.violations[0].depth_mm > 2.9);
    assert_eq!(project.data, before);
    project.put_part("inner".into(), part(2., 30.)).unwrap();
    assert!(
        faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts)
            .unwrap()
            .violations
            .is_empty()
    );
}
