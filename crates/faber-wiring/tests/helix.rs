use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe(pitch: f64) -> Value {
    let rotation = -(20. * std::f64::consts::PI).atan2(pitch).to_degrees();
    json!({"name":"Coil","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"section","geometry":{"op":"sketch","placement":{"translation":[10,0,0],"rotation":[rotation,0,0]},"profile":{"outer":{"kind":"circle","centre":[0,0],"radius":0.5}}}},
        {"id":"path","geometry":{"op":"helix","radius":10,"pitch":pitch,"turns":"turns"}},
        {"id":"solid","geometry":{"op":"sweep_sketch","source":"section","path":"path"}}
    ]}})
}

#[test]
fn helix_sweeps_in_both_directions_and_rebuilds_turn_count() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Helix".into())).unwrap();
    project
        .set_params([("turns".into(), "1".into())].into())
        .unwrap();
    let length = (400. * std::f64::consts::PI.powi(2) + 100.).sqrt();
    let volume = std::f64::consts::PI * 0.25 * length;
    for pitch in [10., -10.] {
        project
            .put_part(
                "coil".into(),
                serde_json::from_value::<Part<Scalar>>(recipe(pitch)).unwrap(),
            )
            .unwrap();
        assert!(
            (project.bodies[0].volume - volume).abs() < 0.001,
            "Volume: {}",
            project.bodies[0].volume
        );
        let (edges, _) = project.feature_topology("coil", "path").unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].length - length).abs() < 1e-5);
    }
    project
        .set_params([("turns".into(), "2".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 2. * volume).abs() < 0.002);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for (field, value) in [
        ("pitch", 0.),
        ("radius", 0.),
        ("radius", -1.),
        ("turns", 0.),
        ("turns", 101.),
    ] {
        let mut invalid = recipe(10.);
        invalid["geometry"]["features"][1]["geometry"][field] = json!(value);
        assert!(
            project
                .put_part(
                    "coil".into(),
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
    assert!((bodies[0].volume - 2. * volume).abs() < 0.002);
}
