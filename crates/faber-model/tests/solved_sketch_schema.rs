use faber_model::{Geometry, Scalar};
use serde_json::{Value, json};

#[test]
fn constrained_sketch_maps_every_dimension_and_keeps_references() {
    let source = json!({"op":"solved_sketch", "plane":"work", "sketch":{
        "points":{"p":["w",2.]}, "entities":{"axis":{"kind":"line","a":"p","b":"q"},"c":{"kind":"circle","centre":"p","radius":"r"},"arc":{"kind":"arc","centre":"p","start":"q","end":"s"}},
        "constraints":[{"kind":"angle","a":"axis","b":"b","degrees":"a"},{"kind":"diameter","entity":"c","value":"d"},{"kind":"symmetric","a":"q","b":"s","about":"axis"}],"construction":["axis"]}});
    let geom: Geometry<Scalar> = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geom).unwrap(), source);
    let numbers = geom
        .map_numbers(&mut |n| match n {
            Scalar::Number(v) => Ok(*v),
            Scalar::Expression(_) => Ok(5.),
        })
        .unwrap();
    let mapped = serde_json::to_value(numbers).unwrap();
    assert_eq!(mapped["sketch"]["constraints"][0]["degrees"], 5.);
    assert_eq!(mapped["sketch"]["entities"]["c"]["radius"], 5.);
    assert_eq!(mapped["sketch"]["points"]["p"], json!([5., 2.]));
    assert_eq!(mapped["sketch"]["construction"], json!(["axis"]));
    assert_eq!(mapped["sketch"]["constraints"][1]["value"], 5.);
}

#[test]
fn offset_sketch_maps_its_signed_distance() {
    let op: Geometry<Scalar> =
        serde_json::from_value(json!({"op":"offset_sketch","sketch":"s","distance":"-gap"}))
            .unwrap();
    let mapped = op.map_numbers(&mut |_| Ok(-2.)).unwrap();
    assert_eq!(
        serde_json::to_value(mapped).unwrap(),
        json!({"op":"offset_sketch","sketch":"s","distance":-2.})
    );
}

#[test]
fn sketch_members_reject_unknown_fields() {
    let g: Value = json!({"op":"solved_sketch","plane":"p","sketch":{"points":{},"entities":{"c":{"kind":"line","a":"a","b":"b","radius":4}},"constraints":[]}});
    assert!(serde_json::from_value::<Geometry>(g).is_err());
}
