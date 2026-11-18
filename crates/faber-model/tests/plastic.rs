use faber_model::{Geometry, Scalar};
use serde_json::json;

#[test]
fn plastic_operations_round_trip_and_resolve_dimensions() {
    let line = json!([[[0, 0], ["span", 0]]]);
    let face = json!({"normal":[0,0,1],"expect":1,"tolerance":0.01});
    for recipe in [
        json!({"op":"rib","body":"base","plane":"xz","lines":line,"thickness":"span","side":"left"}),
        json!({"op":"web","body":"base","plane":"xy","lines":line,"thickness":"span"}),
        json!({"op":"boss","body":"base","plane":"xy","at":[0,0],"diameter":"span","height":8,"ribs":{"count":"span","length":3,"height":3,"thickness":1}}),
        json!({"op":"snap_hook","body":"base","plane":"xy","at":[2,4],"angle_deg":90,"length":"span","width":5,"thickness":2,"undercut":1.5,"hook_length":3}),
        json!({"op":"lip","body":"base","face":face,"loop":"inner","side":"outside","width":"span","height":2}),
        json!({"op":"groove","body":"base","face":face,"loop":"outer","side":"inside","width":"span","depth":2,"clearance":0.15}),
    ] {
        let op: Geometry<Scalar> = serde_json::from_value(recipe.clone()).unwrap();
        let saved = serde_json::to_value(&op).unwrap();
        assert_eq!(saved["op"], recipe["op"]);
        assert_eq!(
            serde_json::from_value::<Geometry<Scalar>>(saved).unwrap(),
            op
        );
        let resolved = op
            .map_numbers(&mut |n| match n {
                Scalar::Number(v) => Ok(*v),
                Scalar::Expression(s) if s == "span" => Ok(14.),
                _ => Err("Unknown dimension.".into()),
            })
            .unwrap();
        let numeric = serde_json::to_string(&resolved).unwrap();
        assert!(numeric.contains("14.0") && !numeric.contains("span"));
        let mut invalid = recipe;
        invalid["unexpected"] = json!(1);
        assert!(serde_json::from_value::<Geometry<Scalar>>(invalid).is_err());
    }
}
