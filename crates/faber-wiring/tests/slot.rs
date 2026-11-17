use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn rotated_slot_can_cut_a_hole_in_a_sketch() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Slot hole".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Plate","colour":[100,140,170],
        "geometry":{"op":"history","output":"solid","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":{
                "outer":{"kind":"rectangle","origin":[-25,-25],"size":[50,50]},
                "holes":[{"kind":"slot","centre":[0,0],"length":30,"width":10,"rotation":35}]
            }}},
            {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,4]}}
        ]}
    }))
    .unwrap();
    project.put_part("plate".into(), part).unwrap();
    let expected = (2500. - 200. - 25. * std::f64::consts::PI) * 4.;
    assert!((project.bodies[0].volume - expected).abs() < 1e-6);
    let (edges, _) = project.feature_topology("plate", "profile").unwrap();
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.radius.is_some_and(|r| (r - 5.).abs() < 1e-8))
            .count(),
        2
    );
}

#[test]
fn slot_sketch_has_round_ends_and_keeps_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Slot".into())).unwrap();
    project
        .set_params([("length".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Slot","colour":[100,140,170],
        "geometry":{"op":"history","output":"solid","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{
                "kind":"slot","centre":[10,20],"length":"length","width":10,"rotation":90
            }}}},
            {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,4]}}
        ]}
    }))
    .unwrap();
    project.put_part("slot".into(), part).unwrap();
    let body = &project.bodies[0];
    assert!((body.volume - (200. + 25. * std::f64::consts::PI) * 4.).abs() < 1e-6);
    for (actual, expected) in body.bounds.iter().flatten().zip([5., 5., 0., 15., 35., 4.]) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    project
        .set_params([("length".into(), "10".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 100. * std::f64::consts::PI).abs() < 1e-6);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for length in ["9", "0", "-1"] {
        assert!(
            project
                .set_params([("length".into(), length.into())].into())
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 100. * std::f64::consts::PI).abs() < 1e-6);
}
