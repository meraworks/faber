use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe() -> Value {
    json!({"name":"Tracked cut","colour":[100,140,170],"geometry":{"op":"history","output":"wall","features":[
        {"id":"base","geometry":{"op":"box","size":[20,20,"height"]}},
        {"id":"drill","geometry":{"op":"cylinder","radius":3,"height":"height + 2"}},
        {"id":"tool","geometry":{"op":"transform","source":"drill","placement":{"translation":[10,10,-1]}}},
        {"id":"cut","geometry":{"op":"difference","left":"base","right":"tool"}},
        {"id":"wall","geometry":{"op":"thicken","source":"cut","thickness":1,"face":{"created_by":"drill","tolerance":0.000001,"expect":1}}}
    ]}})
}

#[test]
fn created_by_tracks_modified_faces_through_transform_and_cut() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Face history".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project
        .put_part(
            "shape".into(),
            serde_json::from_value::<Part<Scalar>>(recipe()).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - 150. * std::f64::consts::PI).abs() < 1e-5);
    project
        .set_params([("height".into(), "40".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 200. * std::f64::consts::PI).abs() < 1e-5);
    let (_, faces) = project.feature_topology("shape", "cut").unwrap();
    let faces = serde_json::to_value(faces).unwrap();
    assert_eq!(
        faces
            .as_array()
            .unwrap()
            .iter()
            .filter(|face| face["created_by"]
                .as_array()
                .is_some_and(|creators| creators.contains(&json!("drill"))))
            .count(),
        1
    );
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for creator in ["missing", "tool", "cut", "base"] {
        let mut invalid = recipe();
        invalid["geometry"]["features"][4]["geometry"]["face"]["created_by"] = json!(creator);
        assert!(
            project
                .put_part(
                    "shape".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 200. * std::f64::consts::PI).abs() < 1e-5);
}

#[test]
fn boolean_history_remains_available_after_part_placement() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Boolean history".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    for (operation, volume, count) in [
        ("union", 12000. + 18. * std::f64::consts::PI, 4),
        ("intersection", 270. * std::f64::consts::PI, 1),
    ] {
        let mut part = recipe();
        part["geometry"]["features"].as_array_mut().unwrap().pop();
        part["geometry"]["features"][3]["geometry"]["op"] = json!(operation);
        part["geometry"]["output"] = json!("cut");
        part["placement"] = json!({"translation":[50,0,0]});
        project
            .put_part(
                "shape".into(),
                serde_json::from_value::<Part<Scalar>>(part).unwrap(),
            )
            .unwrap();
        assert!((project.bodies[0].volume - volume).abs() < 1e-5);
        let (_, faces) = project.topology("shape").unwrap();
        assert_eq!(
            faces
                .iter()
                .filter(|face| face
                    .created_by
                    .as_ref()
                    .is_some_and(|names| names.contains(&"drill".into())))
                .count(),
            count
        );
        assert!(faces.iter().all(|face| face.created_by.is_some()));
    }
}
