use faber_model::Edit;
use serde_json::json;

fn edits(value: serde_json::Value) -> Vec<Edit> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn conditional_dimensions_rebuild_when_a_parameter_crosses_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Conditional".into())).unwrap();
    project.batch(edits(json!([
        {"tool":"set_param","name":"width","expression":"20"},
        {"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":["width",10,"if(width >= 30, 4, 2)"]}}}
    ]))).unwrap();
    assert!((project.bodies[0].volume - 400.).abs() < 1e-6);
    project
        .set_params([("width".into(), "30".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 1200.).abs() < 1e-6);
    project.undo().unwrap();
    assert!((project.bodies[0].volume - 400.).abs() < 1e-6);
}

#[test]
fn mixed_changes_build_once_save_once_and_undo_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Batch".into())).unwrap();
    project.batch(edits(json!([
        {"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"history","features":[{"id":"base","geometry":{"op":"box","size":[10,20,"height"]}}],"output":"base"}}},
        {"tool":"set_param","name":"height","expression":"30"},
        {"tool":"edit_feature","part":"box","id":"base","geometry":{"op":"box","size":[20,20,"height"]}}
    ]))).unwrap();
    assert_eq!(project.data.revision, 1);
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-6);
    project.undo().unwrap();
    assert!(project.data.parts.is_empty());
    assert!(project.data.params.is_empty());
    project.redo().unwrap();
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-6);
}

#[test]
fn failed_batch_keeps_file_memory_and_history_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Batch".into())).unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    for operations in [
        json!([{"tool":"set_param","name":"height","expression":"30"},{"tool":"remove_part","id":"missing"}]),
        json!([{"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,-1]}}}]),
    ] {
        assert!(project.batch(edits(operations)).is_err());
        assert_eq!(
            faber_wiring::project_fingerprint(dir.path()).unwrap(),
            before
        );
        assert!(project.data.parts.is_empty());
        assert!(project.data.params.is_empty());
        assert!(project.undo().is_err());
    }
    assert!(
        serde_json::from_value::<Vec<Edit>>(json!([{"tool":"export","directory":"out"}])).is_err()
    );
}
