use faber_model::{Part, Scalar};
use serde_json::json;

fn part(value: serde_json::Value) -> Part<Scalar> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn box_mass_properties_are_world_values_and_density_adds_mass() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Mass".into())).unwrap();
    let placed = part(json!({
        "name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]},
        "placement":{"translation":[5,0,0]}
    }));
    project.put_part("p".into(), placed).unwrap();
    let report = project.mass_properties("p", None).unwrap();
    assert!((report.volume_mm3 - 6000.).abs() < 1e-7);
    assert!((report.area_mm2 - 2200.).abs() < 1e-7);
    for (actual, expected) in report.centre_of_mass_mm.into_iter().zip([10., 10., 15.]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    assert!(report.mass_g.is_none());
    assert!(report.inertia_kg_mm2.is_none());
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let report = project.mass_properties("p", Some(1.)).unwrap();
    assert!((report.mass_g.unwrap() - 6.).abs() < 1e-9);
    let inertia = report.inertia_kg_mm2.unwrap();
    assert!((inertia[0][0] - 0.65).abs() < 1e-9);
    assert!((inertia[1][1] - 0.5).abs() < 1e-9);
    assert!((inertia[2][2] - 0.25).abs() < 1e-9);
    for (row, column) in [(0, 1), (0, 2), (1, 2)] {
        assert!(inertia[row][column].abs() < 1e-9);
    }
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
}

#[test]
fn cylinder_area_and_volume_are_exact() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Mass".into())).unwrap();
    let cylinder = part(json!({
        "name":"Cylinder","colour":[100,140,170],"geometry":{"op":"cylinder","radius":10,"height":20}
    }));
    project.put_part("p".into(), cylinder).unwrap();
    let report = project.mass_properties("p", None).unwrap();
    assert!((report.volume_mm3 - std::f64::consts::PI * 2000.).abs() < 1e-6);
    let expected = 2. * std::f64::consts::PI * 100. + 2. * std::f64::consts::PI * 10. * 20.;
    assert!((report.area_mm2 - expected).abs() < 1e-6);
}

#[test]
fn unknown_part_and_non_positive_density_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Mass".into())).unwrap();
    let boxed = part(json!({
        "name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}
    }));
    project.put_part("p".into(), boxed).unwrap();
    assert!(project.mass_properties("missing", None).is_err());
    assert!(project.mass_properties("p", Some(0.)).is_err());
    assert!(project.mass_properties("p", Some(-1.)).is_err());
}
#[test]
fn large_density_returns_a_finite_mass_or_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(directory.path(), true, Some("Mass".into())).unwrap();
    for (name, side) in [("small", 10), ("large", 100), ("rod", 1)] {
        let size = if name == "rod" {
            [10000, 1, 1]
        } else {
            [side; 3]
        };
        p.put_part(
            name.into(),
            part(json!({"name":name,"colour":[10,20,30],"geometry":{"op":"box","size":size}})),
        )
        .unwrap();
    }
    let mass = p.mass_properties("small", Some(1e307)).unwrap();
    assert!((mass.mass_g.unwrap() / 1e307 - 1.).abs() < 1e-12);
    assert!(p.mass_properties("large", Some(1e307)).is_err());
    assert!(p.mass_properties("rod", Some(1e306)).is_err());
}
