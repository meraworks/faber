use faber_model::ProjectData;
use serde_json::json;

#[test]
fn material_fields_round_trip_and_old_projects_omit_them() {
    let mut file = json!({"schema":1,"revision":0,"name":"Example","parts":{"p":{
        "name":"Block","geometry":{"op":"box","size":[10,10,10]},"colour":[1,2,3]
    }}});
    let old: ProjectData = serde_json::from_value(file.clone()).unwrap();
    let old = serde_json::to_value(old).unwrap();
    assert!(old.get("materials").is_none());
    assert!(old["parts"]["p"].get("material").is_none());
    file["parts"]["p"]["material"] = json!("custom");
    file["materials"] = json!({"custom":{
        "name":"Test alloy","class":"metal","density_g_cm3":2.0,"youngs_mpa":70000.0,
        "poisson":0.3,"yield_mpa":200.0,"ultimate_mpa":300.0,"conductivity_w_mk":160.0,
        "expansion_um_mk":23.0,"specific_heat_j_kgk":900.0,"layer_strength_factor":1.0,
        "colour":[1,2,3],"roughness":0.4,"metallic":1.0,"source":"Test fixture"
    }});
    let saved =
        serde_json::to_value(serde_json::from_value::<ProjectData>(file.clone()).unwrap()).unwrap();
    assert_eq!(saved["materials"], file["materials"]);
    assert_eq!(saved["parts"]["p"]["material"], "custom");
}
