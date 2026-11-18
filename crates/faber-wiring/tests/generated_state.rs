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

#[test]
fn source_added_to_a_project_with_parts_keeps_them_on_a_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Hand".into())).unwrap();
    project.put_part("a".into(), box_part("A")).unwrap();
    project.put_part("b".into(), box_part("B")).unwrap();
    write(dir.path(), "design.jsonnet", ONE_PART);
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert_eq!(
        project.source_error(),
        Some("design.jsonnet would replace parts: b. Call build to accept.")
    );
    assert!(project.data.parts.contains_key("b"));
    assert_eq!(faber_files::read(dir.path()).unwrap().parts.len(), 2);
    assert!(!dir.path().join(".faber/build.json").exists());
    faber_wiring::build_source(dir.path(), &mut project).unwrap();
    assert_eq!(project.data.parts.keys().collect::<Vec<_>>(), vec!["a"]);
}

#[test]
fn source_cannot_change_a_purchased_part() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Purchased".into())).unwrap();
    let hash = "f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e";
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fastener-check/assets")
        .join(hash);
    project
        .import_with_record(
            "screw".into(),
            "Screw".into(),
            &source,
            true,
            Some(faber_model::Purchased {
                source: "FreeCAD-library".into(),
                licence: "CC BY 3.0".into(),
                designation: "M3 x 10".into(),
                mass_g: None,
            }),
        )
        .unwrap();
    let message = "Part screw is a purchased part. The design source cannot change it.";
    write(
        dir.path(),
        "design.jsonnet",
        "{ parts: { screw: { name: 'Screw', colour: [1, 1, 1], geometry: { op: 'box', size: [3, 3, 10] } } } }",
    );
    faber_wiring::refresh(dir.path(), &mut project).unwrap();
    assert!(project.source_error().is_some_and(|e| e.contains("screw")));
    assert_eq!(
        faber_wiring::build_source(dir.path(), &mut project).unwrap_err(),
        message
    );
    // A source that leaves the purchased part out removes it. That is refused too.
    write(
        dir.path(),
        "design.jsonnet",
        "{ parts: { other: { name: 'Other', colour: [1, 1, 1], geometry: { op: 'box', size: [3, 3, 10] } } } }",
    );
    assert_eq!(
        faber_wiring::build_source(dir.path(), &mut project).unwrap_err(),
        message
    );
    assert!(matches!(
        faber_files::read(dir.path()).unwrap().parts["screw"].geometry,
        faber_model::Geometry::Imported { .. }
    ));
}

#[test]
fn failed_first_build_writes_no_manifest() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "design.jsonnet", BROKEN);
    let error = faber_wiring::design_source().build(dir.path()).unwrap_err();
    assert!(!error.is_empty());
    assert!(!dir.path().join("faber.json").exists());
    assert!(!dir.path().join("placement.json").exists());
}
