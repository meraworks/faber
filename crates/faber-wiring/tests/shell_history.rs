use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn shell_keeps_outer_face_origins_and_names_inner_faces() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shell history".into())).unwrap();
    project
        .set_params([("wall".into(), "1".into())].into())
        .unwrap();
    let recipe = json!({"name":"Open box","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
        {"id":"shell","geometry":{"op":"shell","source":"base","faces":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"thickness":"wall"}},
        {"id":"cap","geometry":{"op":"thicken","source":"shell","thickness":0.5,"face":{"created_by":"shell","normal":[0,0,1],"near_centre":[5,10,"wall"],"expect":1,"tolerance":0.000001}}}
    ]}});
    project
        .put_part(
            "box".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    for (wall, volume) in [(1, 72.), (2, 48.)] {
        project
            .set_params([("wall".into(), wall.to_string())].into())
            .unwrap();
        assert!((project.bodies[0].volume - volume).abs() < 1e-5);
        let (_, faces) = project.feature_topology("box", "shell").unwrap();
        assert!(faces.iter().all(|face| face.created_by.is_some()));
        let bottom = faces
            .iter()
            .find(|face| face.centre[2].abs() < 1e-6)
            .unwrap();
        assert_eq!(
            bottom.created_by.as_deref(),
            Some(["base".to_owned()].as_slice())
        );
        let inner = faces
            .iter()
            .find(|face| (face.centre[2] - f64::from(wall)).abs() < 1e-6)
            .unwrap();
        assert_eq!(
            inner.created_by.as_deref(),
            Some(["shell".to_owned()].as_slice())
        );
    }
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let mut invalid = recipe;
    invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("base");
    assert!(
        project
            .put_part(
                "box".into(),
                serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
            )
            .is_err()
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 48.).abs() < 1e-5);
}
