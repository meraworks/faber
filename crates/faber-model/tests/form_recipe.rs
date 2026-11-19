use faber_model::{Geometry, Scalar};
use serde_json::json;
#[test]
fn form_coordinates_resolve_but_topology_and_settings_round_trip() {
    let value = json!({"op":"form","cage":{"vertices":[["width",0,0],[1,2,3]],"faces":[[0,1,2]],"creases":[{"edge":[0,1],"sharpness":2.5}]},"as_solid":true,"levels":2,"symmetry":["x","z"]});
    let g: Geometry<Scalar> = serde_json::from_value(value.clone()).unwrap();
    let mapped = g
        .map_numbers(&mut |s| match s {
            Scalar::Number(n) => Ok(*n),
            Scalar::Expression(_) => Ok(12.),
        })
        .unwrap();
    let out = serde_json::to_value(mapped).unwrap();
    assert_eq!(out["cage"]["vertices"][0][0], 12.);
    assert_eq!(out["cage"]["creases"], value["cage"]["creases"]);
    assert_eq!(out["symmetry"], value["symmetry"]);
    assert_eq!(out["levels"], 2);
    assert_eq!(out["as_solid"], true);
    let plain: Geometry<f64> =
        serde_json::from_value(json!({"op":"form","cage":{"vertices":[],"faces":[]}})).unwrap();
    let plain = serde_json::to_value(plain).unwrap();
    assert!(plain["levels"].is_null());
    assert_eq!(plain.get("as_solid"), Some(&serde_json::Value::Bool(false)));
}
