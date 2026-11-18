use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn part(head: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Block","colour":[100,140,170],
        "geometry":{"op":"history","output":"hole","features":[
            {"id":"base","geometry":{"op":"box","size":[20,20,20]}},
            {"id":"hole","geometry":{"op":"hole","source":"base","diameter":"diameter","depth":10,
                "placement":{"translation":[10,10,20],"rotation":[180,0,0]},"head":head}}
        ]}
    }))
    .unwrap()
}

#[test]
fn holes_cut_known_volumes_and_keep_parameter_links() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Holes".into())).unwrap();
    project
        .set_params([("diameter".into(), "4".into())].into())
        .unwrap();
    for (head, removed) in [
        (Value::Null, 40.),
        (json!({"kind":"counterbore","diameter":8,"depth":3}), 76.),
        (
            json!({"kind":"countersink","diameter":8,"angle":90}),
            152. / 3.,
        ),
    ] {
        project.put_part("block".into(), part(head)).unwrap();
        assert!((project.bodies[0].volume - (8000. - removed * std::f64::consts::PI)).abs() < 1e-5);
    }
    project.put_part("block".into(), part(Value::Null)).unwrap();
    project
        .set_params([("diameter".into(), "6".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - (8000. - 90. * std::f64::consts::PI)).abs() < 1e-5);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for head in [
        json!({"kind":"counterbore","diameter":4,"depth":3}),
        json!({"kind":"counterbore","diameter":8,"depth":11}),
        json!({"kind":"countersink","diameter":8,"angle":180}),
        json!({"kind":"countersink","diameter":8,"angle":1}),
    ] {
        assert!(project.put_part("block".into(), part(head)).is_err());
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    let mut missed = serde_json::to_value(part(Value::Null)).unwrap();
    missed["geometry"]["features"][1]["geometry"]["placement"]["translation"] =
        json!([100, 100, 20]);
    let missed: Part<Scalar> = serde_json::from_value(missed).unwrap();
    assert!(
        project
            .put_part("block".into(), missed)
            .unwrap_err()
            .contains("does not remove material")
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - (8000. - 90. * std::f64::consts::PI)).abs() < 1e-5);
}
