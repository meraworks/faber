use faber_model::{Part, Scalar};
use serde_json::json;
use std::path::Path;

mod common;

fn block(material: &str) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Cube","colour":[10,20,30],
        "geometry":{"op":"box","size":[10,10,10]}, "material":material}))
    .unwrap()
}

#[test]
fn mass_uses_the_part_material() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Mass".into()).unwrap();
    p.put_part("block".into(), block("steel-s235")).unwrap();
    let report = p.mass_properties("block", None).unwrap();
    assert!((report.mass_g.unwrap() - 7.85).abs() < 1e-9);
    assert_eq!(report.material.as_deref(), Some("steel-s235"));
    assert!(
        (p.mass_properties("block", Some(1.0))
            .unwrap()
            .mass_g
            .unwrap()
            - 1.0)
            .abs()
            < 1e-9
    );
}

#[test]
fn project_material_wins_and_bad_edits_do_not_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Mass".into()).unwrap();
    let mut custom = faber_core::materials::get(&p.data, "pla").unwrap();
    custom.density_g_cm3 = 2.;
    p.set_material("pla".into(), custom.clone()).unwrap();
    p.put_part("block".into(), block("pla")).unwrap();
    drop(p);
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    assert!((p.mass_properties("block", None).unwrap().mass_g.unwrap() - 2.).abs() < 1e-9);
    let files = common::project_files(dir.path());
    custom.density_g_cm3 = 0.;
    assert!(p.set_material("pla".into(), custom).is_err());
    assert!(p.remove_material("pla").unwrap_err().contains("block"));
    assert!(p.put_part("bad".into(), block("pett")).is_err());
    assert_eq!(common::project_files(dir.path()), files);
}

#[test]
fn old_part_file_stays_byte_identical() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/trash-bin");
    let dir = tempfile::tempdir().unwrap();
    for (path, bytes) in common::project_files(&source) {
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let destination = dir.path().join(path.strip_prefix(&source).unwrap());
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, bytes).unwrap();
    }
    let before = common::project_files(dir.path());
    let fingerprint = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let p = faber_wiring::open(dir.path(), false, None).unwrap();
    assert!(
        faber_files::layout::write(dir.path(), &p.data)
            .unwrap()
            .is_empty()
    );
    assert_eq!(common::project_files(dir.path()), before);
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        fingerprint
    );
}
