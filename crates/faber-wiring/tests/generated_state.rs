use faber_model::{Edit, Part, Scalar};
use faber_ports::HistoryMove;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const ONE_PART: &str = "{ parts: { a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } } } }";
const BROKEN: &str = "{ parts: { a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [-1, 10, 10] } } } }";

fn write(root: &Path, rel: &str, text: &str) {
    std::fs::write(root.join(rel), text).unwrap();
}

fn box_part(name: &str) -> Part<Scalar> {
    serde_json::from_value(
        json!({"name": name, "colour": [1, 1, 1], "geometry": {"op": "box", "size": [10, 10, 10]}}),
    )
    .unwrap()
}

#[test]
fn plain_project_gets_refusals_when_a_source_appears() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Plain".into())).unwrap();
    assert_eq!(project.source(), None);
    write(dir.path(), "design.jsonnet", ONE_PART);
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert_eq!(project.source(), Some("design.jsonnet"));
    let error = project
        .set_params(BTreeMap::from([("w".to_string(), "5".to_string())]))
        .unwrap_err();
    assert!(error.contains("generated"), "{error}");
}

#[test]
fn deleted_source_clears_the_label() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "design.jsonnet", ONE_PART);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    assert_eq!(project.source(), Some("design.jsonnet"));
    std::fs::remove_file(dir.path().join("design.jsonnet")).unwrap();
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert_eq!(project.source(), None);
    project
        .set_params(BTreeMap::from([("w".to_string(), "5".to_string())]))
        .unwrap();
}

#[test]
fn plain_remove_of_a_joint_part_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Plain".into())).unwrap();
    project.put_part("a".into(), box_part("A")).unwrap();
    project.put_part("b".into(), box_part("B")).unwrap();
    let study = serde_json::from_value(json!({
        "joints": {"hinge": {"kind": "revolute", "parts": ["b"], "origin": [0, 0, 0], "axis": [0, 0, 1]}},
        "gears": {},
        "linkages": {}
    }))
    .unwrap();
    project.batch(vec![Edit::SetMotion { study }]).unwrap();
    let error = project.remove_part("b").unwrap_err();
    assert!(error.contains("motion"), "{error}");
    assert!(project.data.parts.contains_key("b"));
    drop(project);
    let reopened = faber_wiring::open(dir.path(), false, None).unwrap();
    assert!(reopened.data.parts.contains_key("b"));
}

#[test]
fn plain_undo_is_refused_for_a_generated_project() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "design.jsonnet", ONE_PART);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let error = project.undo().unwrap_err();
    assert!(error.contains("history function"), "{error}");
}

#[test]
fn undo_with_a_broken_source_is_refused_and_builds_again_after_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "design.jsonnet", ONE_PART);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut moved = project.data.parts["a"].clone();
    moved.placement.translation = [Scalar::Number(5.), Scalar::Number(0.), Scalar::Number(0.)];
    project
        .put_parts(BTreeMap::from([("a".to_string(), moved)]))
        .unwrap();
    write(dir.path(), "design.jsonnet", BROKEN);
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert!(project.source_error().is_some());
    let error =
        faber_wiring::history_move(dir.path(), &mut project, HistoryMove::Undo).unwrap_err();
    assert!(error.contains("design source has an error"), "{error}");
    assert!(!dir.path().join(".faber/build.json").exists());
    assert_eq!(
        project.data.parts["a"].placement.translation[0],
        Scalar::Number(5.)
    );
    write(dir.path(), "design.jsonnet", ONE_PART);
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert!(project.source_error().is_none());
    assert_eq!(
        faber_files::read(dir.path()).unwrap().parts["a"]
            .placement
            .translation[0],
        Scalar::Number(5.)
    );
    assert!(dir.path().join(".faber/build.json").is_file());
}
