use faber_model::{Component, Edit, Part, ProjectData, Scalar};
use serde_json::{json, to_value};

#[test]
fn component_tree_and_part_membership_round_trip_without_changing_old_parts() {
    let basic = json!({"schema":1,"revision":0,"name":"Bench","parts":{}});
    let mut data: ProjectData = serde_json::from_value(basic).unwrap();
    assert!(to_value(&data).unwrap().get("components").is_none());
    let part: Part<Scalar> =
        serde_json::from_value(json!({"name":"Block","colour":[4,5,6],"placement":{},
        "geometry":{"op":"box","size":[10,2,3]}}))
        .unwrap();
    assert!(to_value(&part).unwrap().get("component").is_none());
    let root: Component = serde_json::from_value(json!({"name":"Bench"})).unwrap();
    let nested:Component=serde_json::from_value(json!({"name":"Rail","parent":"base","placement":{"translation":[10,0,0],"rotation":[0,0,90]}})).unwrap();
    data.components.insert("base".into(), root);
    data.components.insert("rail".into(), nested.clone());
    let mut part = part;
    part.component = Some("rail".into());
    data.parts.insert("block".into(), part);
    assert_eq!(
        data,
        serde_json::from_value(to_value(&data).unwrap()).unwrap()
    );
    for op in [
        json!({"tool":"set_component","id":"rail","component":nested}),
        json!({"tool":"remove_component","id":"rail","keep_parts":true}),
    ] {
        assert_eq!(
            to_value(serde_json::from_value::<Edit>(op.clone()).unwrap()).unwrap()["id"],
            op["id"]
        );
    }
}
