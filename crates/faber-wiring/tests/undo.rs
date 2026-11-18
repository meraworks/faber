use faber_model::{Part, Scalar};
use serde_json::json;

fn part(height: f64) -> Part<Scalar> {
    serde_json::from_value(
        json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,height]}}),
    )
    .unwrap()
}

#[test]
fn undo_and_redo_survive_reopening_and_new_edits_clear_redo() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Undo".into())).unwrap();
    project.put_part("box".into(), part(30.)).unwrap();
    project.put_part("box".into(), part(40.)).unwrap();
    project.undo().unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
    assert_eq!(project.data.revision, 3);
    drop(project);
    let mut project = faber_wiring::open(directory.path(), false, None).unwrap();
    project.redo().unwrap();
    assert!((project.bodies[0].volume - 8000.).abs() < 1e-6);
    project.undo().unwrap();
    project.put_part("box".into(), part(50.)).unwrap();
    assert!(project.redo().is_err());
    project.undo().unwrap();
    project.undo().unwrap();
    assert!(project.bodies.is_empty());
    assert!(project.undo().is_err());
}

#[test]
fn undo_refuses_to_overwrite_an_external_file_change() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("External edit".into())).unwrap();
    project.put_part("box".into(), part(30.)).unwrap();
    let mut changed = project.data.clone();
    changed.name = "Hand edit".into();
    let bytes = serde_json::to_vec_pretty(&changed).unwrap();
    std::fs::write(directory.path().join("faber.json"), &bytes).unwrap();
    assert!(project.undo().is_err());
    assert_eq!(
        std::fs::read(directory.path().join("faber.json")).unwrap(),
        bytes
    );
}

#[test]
fn a_failed_journal_write_does_not_change_the_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Write failure".into())).unwrap();
    project.put_part("box".into(), part(30.)).unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    std::fs::create_dir(directory.path().join(".faber/history/2.json")).unwrap();
    assert!(project.put_part("box".into(), part(40.)).is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
}

#[test]
fn named_checkpoint_restores_after_reopen_and_restore_can_be_undone() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Checkpoint".into())).unwrap();
    project.put_part("box".into(), part(30.)).unwrap();
    project.checkpoint("first").unwrap();
    assert!(project.checkpoint("first").is_err());
    assert!(project.checkpoint("FIRST").is_err());
    assert!(project.checkpoint("../outside").is_err());
    project.put_part("box".into(), part(40.)).unwrap();
    drop(project);
    let mut project = faber_wiring::open(directory.path(), false, None).unwrap();
    project.restore("first").unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
    project.undo().unwrap();
    assert!((project.bodies[0].volume - 8000.).abs() < 1e-6);
    assert!(project.restore("missing").is_err());
}

#[test]
fn restore_refuses_invalid_geometry_without_changing_the_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Checkpoint".into())).unwrap();
    project.put_part("box".into(), part(30.)).unwrap();
    project.checkpoint("first").unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let mut invalid = project.data.clone();
    invalid.parts.insert("box".into(), part(-10.));
    std::fs::write(
        directory
            .path()
            .join(".faber/checkpoints/checkpoint-first.json"),
        serde_json::to_vec(&invalid).unwrap(),
    )
    .unwrap();
    assert!(project.restore("first").is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
}
