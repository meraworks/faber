use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn revolve_a_circle_sketch_makes_an_exact_torus() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Revolve sketch".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Ring","colour":[100,140,170],"geometry":{"op":"history","output":"ring","features":[
        {"id":"section","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[10,0],"radius":2}}}},
        {"id":"ring","geometry":{"op":"revolve_sketch","source":"section","origin":[0,0,0],"axis":[0,1,0],"angle":360}}
    ]}})).unwrap();
    project.put_part("ring".into(), part).unwrap();
    assert!((project.bodies[0].volume - 80. * std::f64::consts::PI.powi(2)).abs() < 1e-5);
}

#[test]
fn loft_between_circular_sketches_keeps_exact_curves() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Loft sketches".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Transition","colour":[100,140,170],"geometry":{"op":"history","output":"loft","features":[
        {"id":"base","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":5}}}},
        {"id":"top","geometry":{"op":"sketch","placement":{"translation":[0,0,"height"]},"profile":{"outer":{"kind":"circle","centre":[0,0],"radius":10}}}},
        {"id":"loft","geometry":{"op":"loft_sketches","sources":["base","top"],"ruled":true}}
    ]}})).unwrap();
    project.put_part("transition".into(), part).unwrap();
    assert!((project.bodies[0].volume - 1750. * std::f64::consts::PI).abs() < 1e-4);
    project
        .set_params([("height".into(), "60".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 3500. * std::f64::consts::PI).abs() < 1e-4);
    let mut hollow = serde_json::to_value(&project.data.parts["transition"]).unwrap();
    hollow["geometry"]["features"][0]["geometry"]["profile"]["holes"] =
        json!([{"kind":"circle","centre":[0,0],"radius":2}]);
    hollow["geometry"]["features"][1]["geometry"]["profile"]["holes"] =
        json!([{"kind":"circle","centre":[0,0],"radius":4}]);
    let part: Part<Scalar> = serde_json::from_value(hollow.clone()).unwrap();
    project.put_part("transition".into(), part).unwrap();
    assert!((project.bodies[0].volume - 2940. * std::f64::consts::PI).abs() < 1e-4);
    let before = project.data.clone();
    hollow["geometry"]["features"][1]["geometry"]["profile"]["holes"] = json!([]);
    let part: Part<Scalar> = serde_json::from_value(hollow).unwrap();
    assert!(project.put_part("transition".into(), part).is_err());
    assert_eq!(project.data, before);
}
