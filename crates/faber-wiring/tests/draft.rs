use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn drafted_faces_keep_their_source_when_the_angle_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Draft history".into())).unwrap();
    let angle = (1_f64 / 30.).atan().to_degrees();
    project
        .set_params([("angle".into(), angle.to_string())].into())
        .unwrap();
    let recipe = json!({"name":"Cap","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
        {"id":"draft","geometry":{"op":"draft","source":"base","faces":{"area":600,"expect":2,"tolerance":0.000001},"direction":[0,0,1],"origin":[0,0,0],"normal":[0,0,1],"angle":"angle"}},
        {"id":"cap","geometry":{"op":"thicken","source":"draft","thickness":1,"face":{"created_by":"base","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}
    ]}});
    project
        .put_part(
            "body".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    for (value, volume) in [(angle, 160.), (-angle, 240.)] {
        project
            .set_params([("angle".into(), value.to_string())].into())
            .unwrap();
        assert!((project.bodies[0].volume - volume).abs() < 1e-5);
        let (_, faces) = project.feature_topology("body", "draft").unwrap();
        assert_eq!(faces.len(), 6);
        assert!(
            faces
                .iter()
                .all(|face| face.created_by.as_deref() == Some(&["base".into()])),
            "{faces:?}"
        );
    }
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    let mut invalid = recipe;
    invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("draft");
    assert!(
        project
            .put_part(
                "body".into(),
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
    assert!((bodies[0].volume - 240.).abs() < 1e-5);
}

#[test]
fn draft_tapers_selected_faces_about_a_neutral_plane() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Draft".into())).unwrap();
    let angle = (1_f64 / 30.).atan().to_degrees();
    project
        .set_params([("angle".into(), angle.to_string())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Block","colour":[100,140,170],
        "geometry":{"op":"history","output":"draft","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
            {"id":"draft","geometry":{"op":"draft","source":"base",
                "faces":{"area":600,"tolerance":0.000001,"expect":2},
                "direction":[0,0,1],"origin":[0,0,0],"normal":[0,0,1],"angle":"angle"}}
        ]}
    }))
    .unwrap();
    project.put_part("body".into(), part.clone()).unwrap();
    assert!((project.bodies[0].volume - 5400.).abs() < 1e-5);
    project
        .set_params([("angle".into(), (-angle).to_string())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 6600.).abs() < 1e-5);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for value in ["0", "90", "45"] {
        assert!(
            project
                .set_params([("angle".into(), value.into())].into())
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    let mut moved = part;
    let faber_model::Geometry::History { features, .. } = &mut moved.geometry else {
        unreachable!()
    };
    let faber_model::Geometry::Draft { origin, .. } = &mut features[1].geometry else {
        unreachable!()
    };
    origin[2] = Scalar::Number(15.);
    project.put_part("body".into(), moved).unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-5);
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 6000.).abs() < 1e-5);
}
