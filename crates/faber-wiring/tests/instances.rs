use faber_core::{Motion, Project};
use faber_model::{Edit, Part, Scalar};
use serde_json::json;
fn design() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Copies".into())).unwrap();
    let source:Part<Scalar>=serde_json::from_value(json!({"name":"Stock","colour":[20,50,80],"geometry":{"op":"history","features":[{"id":"stock","geometry":{"op":"box","size":[1,2,3]}}],"output":"stock"}})).unwrap();
    p.put_part("a".into(), source).unwrap();
    (dir, p)
}
fn copy(p: &mut Project, id: &str) {
    p.move_linked("a", &Motion::Translate([5., 0., 0.]), id)
        .unwrap();
}
fn change(p: &mut Project, id: &str, width: f64) -> Result<(), String> {
    p.batch(vec![serde_json::from_value(json!({"tool":"edit_feature","part":id,"id":"stock","geometry":{"op":"box","size":[width,2,3]}})).unwrap()])
}
#[test]
fn instance_follows_its_source() {
    let (_dir, mut p) = design();
    copy(&mut p, "b");
    change(&mut p, "a", 10.).unwrap();
    let b = p.bodies.iter().find(|b| b.id == "b").unwrap();
    assert!((b.bounds[1][0] - 15.).abs() < 1e-6);
    assert_eq!(
        change(&mut p, "b", 2.).unwrap_err(),
        "Part b is an instance of a. Change part a."
    );
}
#[test]
fn instance_chain_is_refused() {
    let (_dir, mut p) = design();
    copy(&mut p, "b");
    assert_eq!(
        p.move_linked("b", &Motion::Translate([0.; 3]), "c")
            .unwrap_err(),
        "Part c cannot be an instance of b: b is an instance."
    );
    let rev = p.data.revision;
    let mut a = p.data.parts["a"].clone();
    a.geometry = serde_json::from_value(json!({"op":"instance","of":"b"})).unwrap();
    assert!(p.put_part("a".into(), a).is_err());
    assert_eq!(rev, p.data.revision);
}
#[test]
fn source_with_instances_is_not_removed() {
    let (_dir, mut p) = design();
    copy(&mut p, "c");
    copy(&mut p, "b");
    let msg = p
        .batch(vec![Edit::RemovePart { id: "a".into() }])
        .unwrap_err();
    assert_eq!(msg, "Part a has instances: b, c.");
}
#[test]
fn make_independent_copies_the_recipe() {
    let (_dir, mut p) = design();
    copy(&mut p, "b");
    let place = p.data.parts["b"].placement.clone();
    p.make_independent("b").unwrap();
    assert_eq!(p.data.parts["b"].geometry, p.data.parts["a"].geometry);
    assert_eq!(p.data.parts["b"].placement, place);
    change(&mut p, "a", 8.).unwrap();
    assert!((p.bodies.iter().find(|b| b.id == "b").unwrap().volume - 6.).abs() < 1e-6);
}
#[test]
fn four_screws_from_one_purchased_part() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Fasteners".into())).unwrap();
    let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fastener-check/assets/f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e");
    p.import_with_record(
        "a".into(),
        "Screw".into(),
        &path,
        false,
        Some(faber_model::Purchased {
            designation: "M3 x 10".into(),
            source: "FreeCAD-library".into(),
            licence: "CC BY 3.0".into(),
            mass_g: Some(1.),
        }),
    )
    .unwrap();
    for id in ["b", "c", "d"] {
        copy(&mut p, id);
    }
    assert_eq!(p.bodies.len(), 4);
    let rev = p.data.revision;
    assert!(p.make_independent("b").unwrap_err().contains("purchased"));
    assert!(
        p.move_part("b", Motion::Translate([0.; 3]), Some("other"))
            .is_err()
    );
    assert_eq!(p.data.revision, rev);
    let mut forged = p.data.parts["b"].clone();
    forged.geometry = serde_json::from_value(json!({"op":"box","size":[1,1,1]})).unwrap();
    assert_eq!(
        p.put_part("b".into(), forged).unwrap_err(),
        "Part b is an instance of a. Change part a."
    );
    let rows = faber_core::bom::bom(&p.data, &p.bodies, None, true);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].quantity, 4);
    assert_eq!(rows[0].mass_g, Some(4.));
    assert!(
        matches!(&rows[0].source,faber_core::bom::BomSource::Purchased{designation,..} if designation=="M3 x 10")
    );
}
