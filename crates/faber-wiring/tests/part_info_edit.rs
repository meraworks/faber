use faber_core::{Operation, PartInfo};
use faber_model::{Edit, Part, Scalar};
use serde_json::json;

fn box_part(note: &str) -> Part<Scalar> {
    serde_json::from_value(json!({
        "name": "Box",
        "colour": [100, 140, 170],
        "group": "Frame",
        "note": note,
        "geometry": {"op": "box", "size": [10, 20, 30]}
    }))
    .unwrap()
}

fn edit(value: serde_json::Value) -> Operation {
    Operation::from(serde_json::from_value::<Edit>(value).unwrap())
}

#[test]
fn set_part_info_changes_note_and_colour_and_keeps_the_volume() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Info".into())).unwrap();
    project.put_part("box".into(), box_part("first")).unwrap();
    let volume = project.bodies[0].volume;
    let revision = project.data.revision;
    project
        .set_part_info(PartInfo {
            id: "box".into(),
            note: Some("second".into()),
            colour: Some([10, 20, 30]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(project.data.revision, revision + 1);
    assert_eq!(project.data.parts["box"].note, "second");
    assert_eq!(project.data.parts["box"].colour, [10, 20, 30]);
    assert_eq!(project.data.parts["box"].name, "Box");
    assert_eq!(project.data.parts["box"].group, "Frame");
    assert!((project.bodies[0].volume - volume).abs() < 1e-9);
    assert_eq!(project.bodies[0].colour, [10, 20, 30]);
    let saved = faber_files::read(dir.path()).unwrap();
    assert_eq!(saved.parts["box"].note, "second");
    assert_eq!(saved.parts["box"].colour, [10, 20, 30]);
    assert_eq!(saved.revision, revision + 1);
}

#[test]
fn set_part_info_is_one_undo_step_and_redo_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Undo".into())).unwrap();
    project.put_part("box".into(), box_part("first")).unwrap();
    project
        .set_part_info(PartInfo {
            id: "box".into(),
            name: Some("Lid".into()),
            note: Some("second".into()),
            ..Default::default()
        })
        .unwrap();
    project.undo().unwrap();
    assert_eq!(project.data.parts["box"].note, "first");
    assert_eq!(project.data.parts["box"].name, "Box");
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
    project.redo().unwrap();
    assert_eq!(project.data.parts["box"].note, "second");
    assert_eq!(project.data.parts["box"].name, "Lid");
}

#[test]
fn set_part_info_refuses_unknown_ids_and_empty_changes_without_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Refuse".into())).unwrap();
    project.put_part("box".into(), box_part("first")).unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let unknown = project.set_part_info(PartInfo {
        id: "missing".into(),
        note: Some("x".into()),
        ..Default::default()
    });
    assert!(unknown.is_err());
    let empty = project.set_part_info(PartInfo {
        id: "box".into(),
        ..Default::default()
    });
    assert!(empty.is_err());
    let blank_name = project.set_part_info(PartInfo {
        id: "box".into(),
        name: Some("  ".into()),
        ..Default::default()
    });
    assert!(blank_name.is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    assert_eq!(project.data.parts["box"].note, "first");
}

#[test]
fn batch_accepts_set_part_info_with_other_edits_as_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Batch".into())).unwrap();
    project.put_part("box".into(), box_part("first")).unwrap();
    project
        .batch_operations(
            vec![
                Operation::set_part_info(PartInfo {
                    id: "box".into(),
                    note: Some("batched".into()),
                    group: Some("Shell".into()),
                    ..Default::default()
                }),
                edit(json!({"tool": "set_param", "name": "unused", "expression": "1"})),
            ],
            false,
        )
        .unwrap();
    assert_eq!(project.data.parts["box"].note, "batched");
    assert_eq!(project.data.parts["box"].group, "Shell");
    assert_eq!(project.data.params["unused"], "1");
    project.undo().unwrap();
    assert_eq!(project.data.parts["box"].note, "first");
    assert!(project.data.params.is_empty());
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
}

#[test]
fn batch_refuses_malformed_set_part_info_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Malformed".into())).unwrap();
    project.put_part("box".into(), box_part("first")).unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    for value in [
        json!({"id": "box", "colour": [256, 0, 0]}),
        json!({"id": "box", "colour": [1, 2]}),
        json!({"id": "box", "weight": 3}),
        json!({"id": "box", "note": "x", "tool_extra": 1}),
    ] {
        assert!(serde_json::from_value::<PartInfo>(value).is_err());
    }
    let refused = project.batch_operations(
        vec![
            Operation::set_part_info(PartInfo {
                id: "box".into(),
                note: Some("x".into()),
                ..Default::default()
            }),
            Operation::set_part_info(PartInfo {
                id: "missing".into(),
                note: Some("y".into()),
                ..Default::default()
            }),
        ],
        false,
    );
    assert!(refused.is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    assert_eq!(project.data.parts["box"].note, "first");
}
