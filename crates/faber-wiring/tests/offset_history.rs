use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn offset_tracks_source_faces_and_new_round_transitions() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Offset history".into())).unwrap();
    project
        .set_params([("distance".into(), "0.5".into())].into())
        .unwrap();
    let recipe = json!({"name":"Offset cap","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
        {"id":"base","geometry":{"op":"box","size":[20,20,30]}},
        {"id":"offset","geometry":{"op":"offset","source":"base","distance":"distance"}},
        {"id":"cap","geometry":{"op":"thicken","source":"offset","thickness":1,"face":{"created_by":"base","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}
    ]}});
    project
        .put_part(
            "box".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    for (distance, area, face_count, new_count) in [
        (-0.5, 361., 6, 0),
        (-1., 324., 6, 0),
        (0.5, 400., 26, 20),
        (1., 400., 26, 20),
    ] {
        project
            .set_params([("distance".into(), distance.to_string())].into())
            .unwrap();
        assert!((project.bodies[0].volume - area).abs() < 1e-5);
        assert!((project.bodies[0].bounds[0][2] - 30. - distance).abs() < 1e-5);
        let (_, faces) = project.feature_topology("box", "offset").unwrap();
        assert_eq!(faces.len(), face_count);
        assert_eq!(
            faces
                .iter()
                .filter(|face| face.created_by.as_deref() == Some(&["base".into()]))
                .count(),
            6
        );
        assert_eq!(
            faces
                .iter()
                .filter(|face| face.created_by.as_deref() == Some(&["offset".into()]))
                .count(),
            new_count
        );
    }
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    let mut invalid = recipe;
    invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("offset");
    assert!(
        project
            .put_part(
                "box".into(),
                serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(directory.path().join("faber.json")).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 400.).abs() < 1e-5);
    assert!((bodies[0].bounds[0][2] - 31.).abs() < 1e-5);
}
