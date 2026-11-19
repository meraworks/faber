use faber_model::study::{Study, StudyKind, ThermalCondition};
use serde_json::{Value, json};

fn read(fields: Value) -> Result<Study, serde_json::Error> {
    let mut value = json!({"part":"bar","fixtures":[],"loads":[]});
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(value)
}
#[test]
fn old_static_study_reads_as_static() {
    assert_eq!(read(json!({})).unwrap().kind, StudyKind::Static);
    assert_eq!(
        read(json!({"kind":"static"})).unwrap().kind,
        StudyKind::Static
    );
}
#[test]
fn study_kinds_round_trip_as_flat_records() {
    for fields in [
        json!({"kind":"modal","modes":3}),
        json!({"kind":"thermal","stress":true,"reference_c":20,"conditions":[{"kind":"power","face":{"expect":1,"tolerance":0.01},"watts":5.0}]}),
        json!({"kind":"shape","target_volume":0.5,"keep":[],"symmetry":["middle"]}),
    ] {
        let study = read(fields.clone()).unwrap();
        if let StudyKind::Thermal { conditions, .. } = &study.kind {
            assert!(matches!(&conditions[0], ThermalCondition::Power{watts,..} if *watts==5.));
        }
        let value = serde_json::to_value(&study).unwrap();
        assert_eq!(value["kind"], fields["kind"]);
        assert_eq!(serde_json::from_value::<Study>(value).unwrap(), study);
    }
}
#[test]
fn a_study_refuses_unknown_fields_and_fields_of_another_kind() {
    for fields in [
        json!({"modes":2}),
        json!({"kind":"static","watts":12}),
        json!({"kind":"modal","modes":3,"keep":[]}),
        json!({"kind":"thermal","stress":true,"reference_c":5,"conditions":[{"kind":"temperature","face":{},"watts":3}]}),
        json!({"kind":"shape","target_volume":0.5,"keep":[],"symmetry":[],"typo":1}),
        json!({"kind":"dynamic"}),
    ] {
        assert!(read(fields.clone()).is_err(), "{fields}");
    }
}
