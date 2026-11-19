use faber_model::{Geometry, Scalar};
use serde_json::{Value, json};
fn sheet() -> Value {
    json!({"op":"sheet_metal","rule":{"thickness":"t","bend_radius":2,"relief":{"kind":"rectangular","width":1,"depth":3}},
    "base":{"outer":{"kind":"rectangle","origin":[0,0],"size":[50,40]}},
    "flanges":[{"id":"side","on":"base","edge":2,"length":"leg","angle_deg":-90,"inset_end":1,
        "holes":[{"kind":"circle","centre":[6,8],"radius":2}]}]})
}
#[test]
fn sheet_metal_round_trip_preserves_parameterised_dimensions() {
    let original: Geometry<Scalar> = serde_json::from_value(sheet()).unwrap();
    let encoded = serde_json::to_value(&original).unwrap();
    assert_eq!(encoded["op"], "sheet_metal");
    assert_eq!(
        serde_json::from_value::<Geometry<Scalar>>(encoded).unwrap(),
        original
    );
    let resolved = original
        .map_numbers(&mut |v| {
            Ok(match v {
                Scalar::Number(n) => *n,
                Scalar::Expression(s) if s == "t" => 1.5,
                Scalar::Expression(s) if s == "leg" => 30.,
                _ => return Err("Unknown parameter".into()),
            })
        })
        .unwrap();
    let value = serde_json::to_value(resolved).unwrap();
    assert_eq!(value["rule"]["thickness"], 1.5);
    assert_eq!(value["flanges"][0]["length"], 30.);
    assert_eq!(value["flanges"][0]["holes"][0]["radius"], 2.);
}
#[test]
fn unknown_flange_fields_are_refused() {
    let mut value = sheet();
    value["flanges"][0]["bend_position"] = json!(2);
    assert!(
        serde_json::from_value::<Geometry<Scalar>>(value)
            .unwrap_err()
            .to_string()
            .contains("bend_position")
    );
}
