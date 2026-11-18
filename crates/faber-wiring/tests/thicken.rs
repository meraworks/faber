use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn thicken_a_selected_curved_face_preserves_its_radius() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Curved thicken".into())).unwrap();
    project
        .set_params([("thickness".into(), "2".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Tube","colour":[100,140,170],
        "geometry":{"op":"history","output":"wall","features":[
            {"id":"base","geometry":{"op":"cylinder","radius":5,"height":20}},
            {"id":"wall","geometry":{"op":"thicken","source":"base","thickness":"thickness",
                "face":{"area":200. * std::f64::consts::PI,"tolerance":0.000001,"expect":1}}}
        ]}
    }))
    .unwrap();
    project.put_part("tube".into(), part).unwrap();
    assert!((project.bodies[0].volume - 480. * std::f64::consts::PI).abs() < 1e-5);
    project
        .set_params([("thickness".into(), "-2".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 320. * std::f64::consts::PI).abs() < 1e-5);
}

#[test]
fn thicken_preserves_sketch_holes_in_both_directions() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Thicken".into())).unwrap();
    project
        .set_params([("thickness".into(), "3".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Plate","colour":[100,140,170],
        "geometry":{"op":"history","output":"solid","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":{
                "outer":{"kind":"rectangle","origin":[0,0],"size":[20,20]},
                "holes":[{"kind":"rectangle","origin":[5,5],"size":[10,10]}]}}},
            {"id":"solid","geometry":{"op":"thicken","source":"profile","thickness":"thickness"}}
        ]}
    }))
    .unwrap();
    project.put_part("body".into(), part).unwrap();
    for thickness in [3_f64, -3.] {
        project
            .set_params([("thickness".into(), thickness.to_string())].into())
            .unwrap();
        let body = &project.bodies[0];
        assert!((body.volume - 900.).abs() < 1e-6);
        assert!((body.bounds[0][2] - thickness.min(0.)).abs() < 1e-5);
        assert!((body.bounds[1][2] - thickness.max(0.)).abs() < 1e-5);
    }
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    assert!(
        project
            .set_params([("thickness".into(), "0".into())].into())
            .is_err()
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 900.).abs() < 1e-6);
}
