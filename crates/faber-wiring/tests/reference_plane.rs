use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe() -> Value {
    json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"work","geometry":{"op":"plane","placement":{"translation":[0,0,"height"]}}},
        {"id":"profile","geometry":{"op":"sketch_on_plane","plane":"work","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[2,3]}}}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,4]}}
    ]}})
}

#[test]
fn named_plane_rebuilds_attached_sketches_and_refuses_wrong_references() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Reference plane".into())).unwrap();
    project
        .set_params([("height".into(), "5".into())].into())
        .unwrap();
    project
        .put_part(
            "plate".into(),
            serde_json::from_value::<Part<Scalar>>(recipe()).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - 24.).abs() < 1e-6);
    assert!((project.bodies[0].bounds[0][2] - 5.).abs() < 1e-6);
    project
        .set_params([("height".into(), "12".into())].into())
        .unwrap();
    assert!((project.bodies[0].bounds[0][2] - 12.).abs() < 1e-6);
    let mut rotated = recipe();
    rotated["geometry"]["features"][0]["geometry"]["placement"]["rotation"] = json!([90, 0, 0]);
    rotated["geometry"]["features"][1]["geometry"]["placement"] = json!({"translation":[1,2,0]});
    rotated["geometry"]["features"][2]["geometry"]["vector"] = json!([0, -4, 0]);
    project
        .put_part(
            "plate".into(),
            serde_json::from_value::<Part<Scalar>>(rotated).unwrap(),
        )
        .unwrap();
    let expected = [[1., -4., 14.], [3., 0., 17.]];
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!(
            (actual - expected).abs() < 1e-6,
            "Bounds: {:?}",
            project.bodies[0].bounds
        );
    }
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for reference in ["missing", "profile", "pad"] {
        let mut invalid = recipe();
        invalid["geometry"]["features"][1]["geometry"]["plane"] = json!(reference);
        assert!(
            project
                .put_part(
                    "plate".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    let mut invalid = recipe();
    invalid["geometry"]["features"][0]["geometry"] = json!({"op":"box","size":[1,1,1]});
    assert!(
        project
            .put_part(
                "plate".into(),
                serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(directory.path().join("faber.json")).unwrap(),
        before
    );
    for (pointer, value) in [
        ("/geometry/output", json!("work")),
        (
            "/geometry/features/0/geometry/placement/translation",
            json!([0, 0, 1000001]),
        ),
        ("/geometry/features/2/geometry/source", json!("work")),
    ] {
        let mut invalid = recipe();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            project
                .put_part(
                    "plate".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 24.).abs() < 1e-6);
    assert!((bodies[0].bounds[0][2] - 14.).abs() < 1e-6);
}
