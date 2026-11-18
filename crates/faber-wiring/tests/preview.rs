use faber_model::{Edit, Part, Scalar};
use serde_json::json;

#[test]
fn preview_builds_candidate_without_saving_or_adding_undo_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Preview".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,"height"]}})).unwrap();
    project.put_part("box".into(), part).unwrap();
    let before = project.data.clone();
    let saved = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let edit = Edit::SetParam {
        name: "height".into(),
        expression: "60".into(),
    };
    let (candidate, bodies) = project.preview_batch(vec![edit.clone()], false).unwrap();
    assert_eq!(candidate.revision, before.revision + 1);
    assert!((bodies[0].volume - 12000.).abs() < 1e-5);
    assert_eq!(project.data, before);
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-5);
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        saved
    );
    assert!(
        project
            .preview_batch(
                vec![Edit::SetParam {
                    name: "height".into(),
                    expression: "-1".into()
                }],
                false
            )
            .is_err()
    );
    assert_eq!(project.data, before);
    project.batch(vec![edit]).unwrap();
    assert_eq!(project.data, candidate);
    project.undo().unwrap();
    assert_eq!(project.data.parts, before.parts);
    assert_eq!(project.data.params, before.params);
    project.undo().unwrap();
    assert!(project.data.parts.is_empty());
}

#[test]
fn strict_preview_uses_checks_without_saving_rejected_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Strict preview".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(
        json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]}}),
    )
    .unwrap();
    project.put_part("box".into(), part.clone()).unwrap();
    let saved = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let operations = vec![Edit::PutPart {
        id: "copy".into(),
        part,
    }];
    assert!(project.preview_batch(operations.clone(), true).is_err());
    assert_eq!(project.preview_batch(operations, false).unwrap().1.len(), 2);
    assert_eq!(project.bodies.len(), 1);
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        saved
    );
}
