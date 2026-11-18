use faber_model::{Part, Scalar};
use serde_json::json;
const SHELL: f64 = 40. * 40. * 20. - 36. * 36. * 18.;
fn tray(op: &str, edge: &str, side: &str, width: f64) -> Part<Scalar> {
    let mut feature = json!({"op":op,"body":"shell","face":{"normal":[0,0,1],"near_centre":[20,20,20],"tolerance":0.001,"expect":1},"loop":edge,"side":side,"width":width});
    if op == "lip" {
        feature["height"] = json!(2);
    } else {
        feature["depth"] = json!(2);
        feature["clearance"] = json!(0.15);
    }
    serde_json::from_value(json!({"name":"Tray","colour":[150,160,170],"geometry":{"op":"history","output":"rim","features":[
        {"id":"base","geometry":{"op":"box","size":[40,40,20]}},
        {"id":"shell","geometry":{"op":"shell","source":"base","thickness":2,"faces":{"normal":[0,0,1],"expect":1,"tolerance":0.00001}}},
        {"id":"rim","geometry":feature}
    ]}})).unwrap()
}
fn lip_growth(edge: &str, side: &str, expected: f64) {
    let d = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(d.path(), true, Some("Rim".into())).unwrap();
    p.put_part("tray".into(), tray("lip", edge, side, 1.))
        .unwrap();
    let v = p.bodies[0].volume - SHELL;
    assert!((v - expected).abs() < expected * 0.005, "{v} != {expected}");
}
#[test]
fn lip_on_the_outer_edge() {
    lip_growth("outer", "inside", 312.);
}
#[test]
fn lip_follows_the_inner_loop() {
    lip_growth("inner", "outside", 296.);
}
#[test]
fn width_larger_than_the_rim_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(d.path(), true, Some("Rim".into())).unwrap();
    let err = p
        .put_part("tray".into(), tray("lip", "outer", "inside", 3.))
        .unwrap_err();
    assert!(err.contains("width") || err.contains("rim"), "{err}");
}
#[test]
fn groove_takes_the_lip() {
    let d = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(d.path(), true, Some("Case".into())).unwrap();
    p.put_part("base".into(), tray("lip", "outer", "inside", 1.))
        .unwrap();
    let mut lid = tray("groove", "outer", "inside", 1.);
    lid.placement.translation = [0., 40., 40.15].map(Scalar::Number);
    lid.placement.rotation = [180., 0., 0.].map(Scalar::Number);
    p.put_part("lid".into(), lid).unwrap();
    let report = faber_wiring::interference(&p.bodies, 0.01, &p.data.contacts).unwrap();
    assert!(report.violations.is_empty());
    let dist = p.measure_distance("base", "lid").unwrap().distance_mm;
    assert!((dist - 0.15).abs() < 0.02, "{dist}");
}
