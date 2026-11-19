use faber_model::{Geometry, Scalar};
use serde_json::json;
#[test]
fn all_surface_recipe_types_round_trip_and_resolve_dimensions() {
    let edge = json!({"tolerance":"dim","expect":1,"near_centre":["dim",0,0]});
    let face = json!({"tolerance":"dim","expect":1});
    for v in [
        json!({"op":"surface_extrude","profile":"s","distance":"dim","direction":[0,0,"dim"]}),
        json!({"op":"surface_revolve","profile":"p","axis":"a","angle_deg":"dim"}),
        json!({"op":"surface_loft","sections":["a","b"],"ruled":true}),
        json!({"op":"surface_sweep","profile":"s","path":"p"}),
        json!({"op":"patch","boundary":[{"kind":"curve","feature":"curve"},{"kind":"edge","feature":"base","edge":edge}],"tangent_to":["cap"]}),
        json!({"op":"trim","surface":"skin","tool":"c","keep":{"near":[1,"dim",3]}}),
        json!({"op":"extend","surface":"skin","edge":edge,"distance":"dim"}),
        json!({"op":"stitch","surfaces":["l","r"],"tolerance":"dim","as_solid":true}),
        json!({"op":"split_body","body":"body","tool":"skin","keep":"negative"}),
        json!({"op":"replace_face","body":"body","face":face,"surface":"skin"}),
    ] {
        let g: Geometry<Scalar> = serde_json::from_value(v.clone()).unwrap();
        let round: Geometry<Scalar> =
            serde_json::from_value(serde_json::to_value(&g).unwrap()).unwrap();
        assert_eq!(g, round);
        let n = g
            .map_numbers(&mut |p| {
                Ok(match p {
                    Scalar::Number(n) => *n,
                    Scalar::Expression(s) if s == "dim" => 3.5,
                    _ => return Err("Unknown parameter".into()),
                })
            })
            .unwrap();
        let encoded = serde_json::to_value(&n).unwrap();
        assert_eq!(encoded["op"], v["op"]);
        assert!(!encoded.to_string().contains("dim"));
        if v.to_string().contains("dim") {
            assert!(encoded.to_string().contains("3.5"));
        }
    }
}
#[test]
fn surface_selectors_reject_unknown_fields_and_tags() {
    for v in [
        json!({"op":"patch","boundary":[{"kind":"point","feature":"s"}]}),
        json!({"op":"patch","boundary":[{"kind":"curve","feature":"s","near":[0,0,0]}]}),
        json!({"op":"trim","surface":"s","tool":"p","keep":{"near":[0,0,0],"flip":true}}),
        json!({"op":"stitch","surfaces":["s"],"as_solid":"yes"}),
    ] {
        assert!(serde_json::from_value::<Geometry<Scalar>>(v).is_err());
    }
}
