use faber_model::{Edit, ProjectData};
use serde_json::{Value, from_value, json, to_value};

#[test]
fn named_configuration_preserves_expressions_and_suppressed_feature_ids() {
    let legacy = json!({"schema":1,"revision":0,"name":"Panel","parts":{}});
    let mut value = legacy.clone();
    value["configurations"] = json!({"thin":{"params":{"wall":"2 * t"},
        "suppressed":[{"part":"case","feature":"lip"}],"note":"Test fit"}});
    value["active_configuration"] = Value::String("thin".into());
    let data: ProjectData = from_value(value.clone()).unwrap();
    assert_eq!(
        to_value(data).unwrap()["configurations"],
        value["configurations"]
    );
    let data: ProjectData = from_value(legacy).unwrap();
    let saved = to_value(data).unwrap();
    assert!(saved.get("configurations").is_none());
    assert!(saved.get("active_configuration").is_none());
    for cmd in [
        json!({"tool":"use_configuration","name":"thin"}),
        json!({"tool":"set_configuration","name":"thin","configuration":value["configurations"]["thin"]}),
        json!({"tool":"remove_configuration","name":"thin"}),
    ] {
        assert_eq!(
            to_value(from_value::<Edit>(cmd.clone()).unwrap()).unwrap(),
            cmd
        );
    }
}
