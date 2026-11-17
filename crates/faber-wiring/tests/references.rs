use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe() -> Value {
    json!({"name":"Ring","colour":[100,140,170],"geometry":{"op":"history","output":"placed","features":[
        {"id":"axis","geometry":{"op":"axis","origin":["axis_x",0,0],"direction":[0,0,2]}},
        {"id":"point","geometry":{"op":"point","position":["point_x",20,30]}},
        {"id":"profile","geometry":{"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"rectangle","origin":[2,0],"size":[2,5]}}}},
        {"id":"ring","geometry":{"op":"revolve_around","source":"profile","axis":"axis","angle":360}},
        {"id":"placed","geometry":{"op":"place_at_point","source":"ring","point":"point"}}
    ]}})
}

#[test]
fn axis_and_point_references_rebuild_dependent_solids() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("References".into())).unwrap();
    project
        .set_params(
            [
                ("axis_x".into(), "1".into()),
                ("point_x".into(), "10".into()),
            ]
            .into(),
        )
        .unwrap();
    project
        .put_part(
            "ring".into(),
            serde_json::from_value::<Part<Scalar>>(recipe()).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - 40. * std::f64::consts::PI).abs() < 1e-5);
    let expected = [[8., 17., 30.], [14., 23., 35.]];
    for (a, b) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!(
            (a - b).abs() < 1e-5,
            "Bounds: {:?}",
            project.bodies[0].bounds
        );
    }
    project
        .set_params(
            [
                ("axis_x".into(), "0".into()),
                ("point_x".into(), "50".into()),
            ]
            .into(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - 60. * std::f64::consts::PI).abs() < 1e-5);
    assert!((project.bodies[0].bounds[0][0] - 46.).abs() < 1e-5);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for (pointer, value) in [
        ("/geometry/features/0/geometry/direction", json!([0, 0, 0])),
        (
            "/geometry/features/1/geometry/position",
            json!([1000001, 0, 0]),
        ),
        ("/geometry/features/3/geometry/axis", json!("point")),
        ("/geometry/features/4/geometry/point", json!("axis")),
        ("/geometry/features/3/geometry/source", json!("axis")),
        ("/geometry/features/4/geometry/point", json!("missing")),
        ("/geometry/output", json!("axis")),
        ("/geometry/output", json!("point")),
    ] {
        let mut invalid = recipe();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            project
                .put_part(
                    "ring".into(),
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
    assert!((bodies[0].volume - 60. * std::f64::consts::PI).abs() < 1e-5);
    assert!((bodies[0].bounds[0][0] - 46.).abs() < 1e-5);
}
