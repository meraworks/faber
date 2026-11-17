use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn imported_solid_reopens_without_its_source_and_detects_asset_changes() {
    let directory = tempfile::tempdir().unwrap();
    let original_root = directory.path().join("original");
    let mut original = faber_wiring::open(&original_root, true, Some("Original".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(
        json!({"name":"Block","colour":[80,100,120],"geometry":{"op":"box","size":[10,20,30]}}),
    )
    .unwrap();
    original.put_part("block".into(), part).unwrap();
    let output = directory.path().join("output");
    faber_wiring::export_checked(
        &original.bodies,
        &original.data.contacts,
        &output,
        "step",
        false,
    )
    .unwrap();
    let root = directory.path().join("imported");
    let mut project = faber_wiring::open(&root, true, Some("Imported".into())).unwrap();
    project
        .import_part(
            "block".into(),
            "Imported block".into(),
            &output.join("block.step"),
            false,
        )
        .unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-5);
    std::fs::remove_file(output.join("block.step")).unwrap();
    let (_, bodies) = faber_wiring::read(&root).unwrap();
    assert!((bodies[0].volume - 6000.).abs() < 1e-5);
    let imported = project.data.clone();
    project.undo().unwrap();
    assert!(project.bodies.is_empty());
    project.redo().unwrap();
    assert_eq!(project.data.parts, imported.parts);
    let geometry = serde_json::to_value(&project.data.parts["block"].geometry).unwrap();
    let edited: Part<Scalar> = serde_json::from_value(json!({"name":"Imported row","colour":[80,100,120],"geometry":{"op":"history","output":"row","features":[
        {"id":"imported","geometry":geometry},
        {"id":"row","geometry":{"op":"linear_pattern","source":"imported","count":2,"step":[30,0,0]}}
    ]}})).unwrap();
    project.put_part("block".into(), edited).unwrap();
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-5);
    let (_, bodies) = faber_wiring::read(&root).unwrap();
    assert!((bodies[0].volume - 12000.).abs() < 1e-5);
    let saved = std::fs::read(root.join("faber.json")).unwrap();
    let asset = std::fs::read_dir(root.join("assets"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(asset, b"changed").unwrap();
    assert!(faber_wiring::read(&root).is_err());
    assert!(
        project
            .set_params([("unused".into(), "1".into())].into())
            .is_err()
    );
    assert_eq!(std::fs::read(root.join("faber.json")).unwrap(), saved);
}

#[test]
fn malformed_import_keeps_the_saved_project_and_existing_parts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let mut project = faber_wiring::open(&root, true, Some("Invalid import".into())).unwrap();
    let saved = std::fs::read(root.join("faber.json")).unwrap();
    let source = directory.path().join("bad.step");
    std::fs::write(&source, b"not a model").unwrap();
    assert!(
        project
            .import_part("part".into(), "Part".into(), &source, false)
            .is_err()
    );
    assert!(project.bodies.is_empty());
    assert_eq!(std::fs::read(root.join("faber.json")).unwrap(), saved);
}
