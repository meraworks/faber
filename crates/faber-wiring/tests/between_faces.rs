use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn face(normal: [f64; 3]) -> Value {
    json!({"normal":normal,"tolerance":0.000001,"expect":1})
}
fn recipe(operation: &str, second: Value, expect: usize) -> Part<Scalar> {
    let mut dress = json!({"op":operation,"source":"base","edges":{"between":[face([0.,0.,1.]),second],"tolerance":0.000001,"expect":expect}});
    dress[if operation == "fillet" {
        "radius"
    } else {
        "distance"
    }] = json!(2);
    serde_json::from_value(json!({"name":"Dressed box","colour":[100,140,170],"geometry":{"op":"history","output":"dress","features":[
        {"id":"base","geometry":{"op":"box","size":["width",20,30]}},{"id":"dress","geometry":dress}
    ]}})).unwrap()
}

#[test]
fn shared_edge_selection_survives_dimension_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Shared edges".into())).unwrap();
    project
        .set_params([("width".into(), "10".into())].into())
        .unwrap();
    for (operation, removed) in [
        ("fillet", 20. * (4. - std::f64::consts::PI)),
        ("chamfer", 40.),
    ] {
        project
            .put_part("box".into(), recipe(operation, face([1., 0., 0.]), 1))
            .unwrap();
        assert!((project.bodies[0].volume - (6000. - removed)).abs() < 1e-5);
    }
    project
        .set_params([("width".into(), "15".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 8960.).abs() < 1e-5);
    let both_sides = json!({"area":"20 * 30","expect":2,"tolerance":0.000001});
    project
        .put_part("box".into(), recipe("chamfer", both_sides, 2))
        .unwrap();
    assert!((project.bodies[0].volume - 8920.).abs() < 1e-5);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for second in [
        face([0., 0., -1.]),
        face([0., 0., 1.]),
        json!({"normal":[1,0,0],"expect":2,"tolerance":0.000001}),
    ] {
        assert!(
            project
                .put_part("box".into(), recipe("fillet", second, 1))
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 8920.).abs() < 1e-5);
}
