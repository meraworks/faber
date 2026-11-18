use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe(normal: [f64; 3]) -> Value {
    json!({"name":"Face pad","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"base","geometry":{"op":"box","size":["width",20,"height"]}},
        {"id":"work","geometry":{"op":"plane_on_face","source":"base","face":{"normal":normal,"expect":1,"tolerance":0.000001},"offset":3}},
        {"id":"profile","geometry":{"op":"sketch_on_plane","plane":"work","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":1}}}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"profile","vector":normal.map(|v|v*2.)}}
    ]}})
}

#[test]
fn face_plane_tracks_source_dimensions_and_outward_normals() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Face planes".into())).unwrap();
    project
        .set_params(
            [
                ("width".into(), "10".into()),
                ("height".into(), "30".into()),
            ]
            .into(),
        )
        .unwrap();
    project
        .put_part(
            "pad".into(),
            serde_json::from_value::<Part<Scalar>>(recipe([0., 0., 1.])).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].bounds[0][2] - 33.).abs() < 1e-6);
    project
        .set_params(
            [
                ("width".into(), "20".into()),
                ("height".into(), "40".into()),
            ]
            .into(),
        )
        .unwrap();
    for (normal, bounds) in [
        ([0., 0., 1.], [[9., 9., 43.], [11., 11., 45.]]),
        ([0., 1., 0.], [[9., 23., 19.], [11., 25., 21.]]),
        ([-1., 0., 0.], [[-5., 9., 19.], [-3., 11., 21.]]),
        ([0., 0., -1.], [[9., 9., -5.], [11., 11., -3.]]),
    ] {
        project
            .put_part(
                "pad".into(),
                serde_json::from_value::<Part<Scalar>>(recipe(normal)).unwrap(),
            )
            .unwrap();
        assert!((project.bodies[0].volume - 2. * std::f64::consts::PI).abs() < 1e-5);
        for (actual, expected) in project.bodies[0]
            .bounds
            .iter()
            .flatten()
            .zip(bounds.iter().flatten())
        {
            assert!(
                (actual - expected).abs() < 1e-5,
                "Bounds: {:?}",
                project.bodies[0].bounds
            );
        }
    }
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for face in [
        json!({"expect":6,"tolerance":0.000001}),
        json!({"normal":[0,0,1],"expect":2,"tolerance":0.000001}),
    ] {
        let mut invalid = recipe([0., 0., 1.]);
        invalid["geometry"]["features"][1]["geometry"]["face"] = face;
        assert!(
            project
                .put_part(
                    "pad".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    let mut invalid = recipe([0., 0., 1.]);
    invalid["geometry"]["features"][0]["geometry"] =
        json!({"op":"cylinder","radius":5,"height":20});
    invalid["geometry"]["features"][1]["geometry"]["face"] =
        json!({"area":200.*std::f64::consts::PI,"expect":1,"tolerance":0.000001});
    assert!(
        project
            .put_part(
                "pad".into(),
                serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
            )
            .unwrap_err()
            .contains("planar")
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].bounds[0][2] + 5.).abs() < 1e-5);
}

#[test]
fn oblique_face_plane_preserves_the_face_normal_and_centre() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Oblique plane".into())).unwrap();
    project
        .set_params(
            [
                ("width".into(), "20".into()),
                ("height".into(), "40".into()),
            ]
            .into(),
        )
        .unwrap();
    let s = 0.5_f64.sqrt();
    let mut part = recipe([s, 0., s]);
    part["geometry"]["features"].as_array_mut().unwrap().insert(1,json!({"id":"rotated","geometry":{"op":"transform","source":"base","placement":{"rotation":[0,45,0]}}}));
    part["geometry"]["features"][2]["geometry"]["source"] = json!("rotated");
    part["geometry"]["features"][3]["geometry"]["profile"]["outer"]["centre"] = json!([2, 3]);
    project
        .put_part(
            "pad".into(),
            serde_json::from_value::<Part<Scalar>>(part).unwrap(),
        )
        .unwrap();
    let expected = [[54. * s, 12., 30. * s], [58. * s, 14., 34. * s]];
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!(
            (actual - expected).abs() < 1e-5,
            "Bounds: {:?}",
            project.bodies[0].bounds
        );
    }
    assert!((project.bodies[0].volume - 2. * std::f64::consts::PI).abs() < 1e-5);
}
