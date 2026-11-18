use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn offset_expands_round_corners_and_contracts_a_solid() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Offset".into())).unwrap();
    project
        .set_params([("distance".into(), "1".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Box","colour":[100,140,170],
        "geometry":{"op":"history","output":"offset","features":[
            {"id":"base","geometry":{"op":"box","size":[10,10,10]}},
            {"id":"offset","geometry":{"op":"offset","source":"base","distance":"distance"}}
        ]}
    }))
    .unwrap();
    project.put_part("body".into(), part).unwrap();
    let expected = 1600. + (30. + 4. / 3.) * std::f64::consts::PI;
    assert!((project.bodies[0].volume - expected).abs() < 1e-5);
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip([-1., -1., -1., 11., 11., 11.])
    {
        assert!((actual - expected).abs() < 1e-5);
    }
    project
        .set_params([("distance".into(), "-1".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 512.).abs() < 1e-6);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for value in ["0", "-6"] {
        assert!(
            project
                .set_params([("distance".into(), value.into())].into())
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 512.).abs() < 1e-6);
}
