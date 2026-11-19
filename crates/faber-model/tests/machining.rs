use faber_model::ProjectData;
use serde_json::{Value, json};
fn empty() -> Value {
    json!({"schema":1,"name":"Mill","revision":0,"parts":{}})
}
fn job(kind: Value) -> Value {
    let mut job = json!({"id":"op","setup":"s","tool":"t","heights":{"clearance":8,"retract":3,"top":0,"bottom":-5},"stepdown":2});
    for (k, v) in kind.as_object().unwrap() {
        job[k] = v.clone();
    }
    job
}
#[test]
fn old_projects_do_not_gain_machining_data() {
    let v = serde_json::from_value::<ProjectData>(empty()).unwrap();
    assert!(serde_json::to_value(v).unwrap().get("machining").is_none());
}
#[test]
fn all_machining_strategies_round_trip_in_cut_order() {
    let mut p = empty();
    let ops = [
        json!({"kind":"face","stepover":4}),
        json!({"kind":"contour","z":{"z":-2},"side":"outside","stock_to_leave":0.1}),
        json!({"kind":"pocket","floor":{"face":{"near_centre":[0,0,-2],"tolerance":0.001,"expect":1}},"stepover":4,"stock_to_leave":0}),
        json!({"kind":"drill","holes":{"list":[[0,0],[1,1]]},"peck":1}),
        json!({"kind":"drill","holes":"auto"}),
        json!({"kind":"rough3d","stepover":4,"stock_to_leave":0.2}),
        json!({"kind":"parallel3d","stepover":0.5,"angle_deg":30}),
        json!({"kind":"cut2d","kerf":0.25,"lead_in":2,"power":600}),
    ];
    p["machining"] = json!({"tools":{"t":{"kind":"flat","diameter":6,"corner_radius":0,"flute_length":20,"flutes":2,"feed_mm_min":600,"plunge_mm_min":120,"spindle_rpm":12000,"number":1}},"setups":{"s":{"part":"plate","stock":{"kind":"offset","sides":1,"top":2,"bottom":0},"origin":"stock_top_centre","up":[0,0,1]}},"operations":ops.into_iter().map(job).collect::<Vec<_>>()});
    let model: ProjectData = serde_json::from_value(p.clone()).unwrap();
    let enc = serde_json::to_value(&model).unwrap();
    assert_eq!(
        serde_json::from_value::<ProjectData>(enc.clone()).unwrap(),
        model
    );
    assert_eq!(
        enc["machining"]["operations"][3]["holes"]["list"][1][0].as_f64(),
        Some(1.)
    );
    assert_eq!(enc["machining"]["operations"][7]["kind"], "cut2d");
    p["machining"]["operations"][0]["unknown_dim"] = json!(2);
    assert!(serde_json::from_value::<ProjectData>(p).is_err());
}
