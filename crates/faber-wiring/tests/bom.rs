use faber_core::{Project, bom::bom};
use faber_model::{Part, Scalar};
use faber_wiring::bom::csv;
use serde_json::json;
fn assembly() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Materials".into())).unwrap();
    p.set_params([("width".into(), "10".into())].into())
        .unwrap();
    let components = ["root", "sub"].map(|id| {
        serde_json::from_value(json!({"tool":"set_component","id":id,"component":{
            "name":id,"parent":if id=="sub"{Some("root")}else{None}
        }}))
        .unwrap()
    });
    p.batch(components.into()).unwrap();
    for (id, material, component, width) in [
        ("first", "pla", "root", json!("width")),
        ("second", "pla", "sub", json!(10)),
        ("third", "petg", "sub", json!(10)),
    ] {
        let part:Part<Scalar>=serde_json::from_value(json!({"name":"Panel, \"A\"","material":material,"component":component,"colour":[120,80,10],"geometry":{"op":"box","size":[width,10,10]}})).unwrap();
        p.put_part(id.into(), part).unwrap();
    }
    (dir, p)
}
#[test]
fn equal_recipes_group_without_instances_and_materials_stay_separate() {
    let (_dir, p) = assembly();
    let rows = bom(&p.data, &p.bodies, None, true);
    assert_eq!(rows.len(), 2);
    let pla = rows
        .iter()
        .find(|r| r.material.as_deref() == Some("pla"))
        .unwrap();
    assert_eq!(pla.parts, ["first", "second"]);
    assert_eq!(pla.quantity, 2);
    assert!(
        (pla.mass_g.unwrap()
            - 2. * faber_core::materials::get(&p.data, "pla")
                .unwrap()
                .density_g_cm3)
            .abs()
            < 1e-7
    );
}
#[test]
fn csv_has_a_header_and_quoted_names() {
    let (_dir, p) = assembly();
    let text = csv(&bom(&p.data, &p.bodies, None, true));
    assert!(
        text.lines()
            .next()
            .unwrap()
            .to_lowercase()
            .contains("quantity")
    );
    assert!(text.contains("\"Panel, \"\"A\"\"\""));
}
#[test]
fn scope_limits_to_a_component_and_hierarchical_rows_do_not_cross_boundaries() {
    let (_dir, p) = assembly();
    let rows = bom(&p.data, &p.bodies, Some("sub"), true);
    assert!(rows.iter().all(|r| !r.parts.iter().any(|id| id == "first")));
    assert_eq!(rows.iter().map(|r| r.quantity).sum::<u32>(), 2);
    assert_eq!(bom(&p.data, &p.bodies, Some("root"), false).len(), 3);
}
