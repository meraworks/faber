use faber_model::{Edit, Part, Scalar};
use serde_json::{Value, json};

fn boxed(size: [f64; 3], translation: [f64; 3]) -> Part<Scalar> {
    serde_json::from_value(json!({
        "name":"Box","colour":[100,140,170],
        "geometry":{"op":"box","size":size},
        "placement":{"translation":translation}
    }))
    .unwrap()
}

fn stacked_boxes(dir: &tempfile::TempDir) -> faber_core::Project {
    let mut project = faber_wiring::open(dir.path(), true, Some("Contacts".into())).unwrap();
    project
        .put_part("box".into(), boxed([10., 10., 10.], [0., 0., 0.]))
        .unwrap();
    project
        .put_part("lid".into(), boxed([10., 10., 10.], [0., 0., 10.]))
        .unwrap();
    project
}

fn overlapping_boxes(dir: &tempfile::TempDir) -> faber_core::Project {
    let mut project = faber_wiring::open(dir.path(), true, Some("Contacts".into())).unwrap();
    project
        .put_part("box".into(), boxed([10., 10., 10.], [0., 0., 0.]))
        .unwrap();
    project
        .put_part("lid".into(), boxed([10., 10., 10.], [0., 0., 9.]))
        .unwrap();
    project
}

fn declare(project: &mut faber_core::Project, a: &str, b: &str, allowed: bool) {
    project
        .batch(vec![Edit::SetContact {
            a: a.into(),
            b: b.into(),
            allowed,
            note: "Seated lid".into(),
            max_depth_mm: None,
        }])
        .unwrap();
}

#[test]
fn stacked_faces_are_contact_not_interference() {
    let dir = tempfile::tempdir().unwrap();
    let project = stacked_boxes(&dir);
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert_eq!(report.contacts.len(), 1);
    assert!(report.contacts[0].depth_mm <= faber_clash::scan::CONTACT_TOLERANCE_MM);
    let json: Value = serde_json::to_value(&report).unwrap();
    assert_eq!(json["contacts"][0]["kind"], "contact");
    let output = dir.path().join("out");
    let files = faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
}

#[test]
fn one_millimetre_overlap_is_interference_and_export_names_the_parts() {
    let dir = tempfile::tempdir().unwrap();
    let project = overlapping_boxes(&dir);
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert_eq!(report.violations.len(), 1);
    assert!((report.violations[0].depth_mm - 1.).abs() < 1e-6);
    assert!(report.contacts.is_empty());
    let output = dir.path().join("out");
    let error = faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap_err();
    assert!(error.contains("box"), "{error}");
    assert!(error.contains("lid"), "{error}");
    assert!(error.contains("interference"), "{error}");
    assert!(error.contains("depth 1.000 mm"), "{error}");
    assert!(
        error.contains("Declare an intended contact with set_contact, or use force: true."),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn declared_contact_allows_an_overlap_without_violation() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = overlapping_boxes(&dir);
    // A 1 mm press needs a limit above 1 mm.
    project
        .batch(vec![Edit::SetContact {
            a: "lid".into(),
            b: "box".into(),
            allowed: true,
            note: "Seated lid".into(),
            max_depth_mm: Some(1.5),
        }])
        .unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert_eq!(report.allowed_contacts.len(), 1);
    let output = dir.path().join("out");
    let files = faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
}

#[test]
fn declared_contact_changes_only_the_contact_list_and_undo_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    let before = faber_files::read(dir.path()).unwrap();
    declare(&mut project, "box", "lid", true);
    let after = faber_files::read(dir.path()).unwrap();
    assert_eq!(after.parts, before.parts);
    assert_eq!(after.params, before.params);
    assert_eq!(after.name, before.name);
    assert_eq!(after.revision, before.revision + 1);
    let contact = &after.contacts["box|lid"];
    assert_eq!(contact.note, "Seated lid");
    assert_eq!(contact.a, "box");
    assert_eq!(contact.b, "lid");
    // The contacts live in assembly.json. The manifest has none.
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("faber.json")).unwrap()).unwrap();
    assert!(manifest.get("contacts").is_none());
    project.undo().unwrap();
    let undone = faber_files::read(dir.path()).unwrap();
    assert!(undone.contacts.is_empty());
    assert_eq!(undone.parts, before.parts);
    assert!(project.data.contacts.is_empty());
}

#[test]
fn removing_a_part_removes_its_declared_contacts() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    declare(&mut project, "box", "lid", true);
    project.remove_part("lid").unwrap();
    assert!(project.data.contacts.is_empty());
}

#[test]
fn set_contact_refuses_unknown_parts_and_self_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    assert!(
        project
            .batch(vec![Edit::SetContact {
                a: "box".into(),
                b: "missing".into(),
                allowed: true,
                note: String::new(),
                max_depth_mm: None,
            }])
            .is_err()
    );
    assert!(
        project
            .batch(vec![Edit::SetContact {
                a: "box".into(),
                b: "box".into(),
                allowed: true,
                note: String::new(),
                max_depth_mm: None,
            }])
            .is_err()
    );
    assert!(project.data.contacts.is_empty());
}

#[test]
fn separated_boxes_outside_the_band_are_neither_contact_nor_interference() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Contacts".into())).unwrap();
    project
        .put_part("box".into(), boxed([10., 10., 10.], [0., 0., 0.]))
        .unwrap();
    project
        .put_part("lid".into(), boxed([10., 10., 10.], [0., 0., 11.]))
        .unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert!(report.contacts.is_empty());
}

fn lid_at(project: &mut faber_core::Project, z: f64) {
    project
        .put_part("lid".into(), boxed([10., 10., 10.], [0., 0., z]))
        .unwrap();
}

fn declare_with_limit(
    project: &mut faber_core::Project,
    max_depth_mm: Option<f64>,
) -> Result<(), String> {
    project.batch(vec![Edit::SetContact {
        a: "box".into(),
        b: "lid".into(),
        allowed: true,
        note: "Seated lid".into(),
        max_depth_mm,
    }])
}

#[test]
fn declared_touching_pair_has_no_violation() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    declare_with_limit(&mut project, None).unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    let output = dir.path().join("out");
    faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap();
}

#[test]
fn declared_pair_overlap_below_limit_has_no_violation() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    lid_at(&mut project, 9.7);
    declare_with_limit(&mut project, None).unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert_eq!(report.allowed_contacts.len(), 1);
}

#[test]
fn declared_pair_overlap_above_limit_is_a_violation_and_export_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    lid_at(&mut project, 5.);
    declare_with_limit(&mut project, None).unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(report.violations[0].depth_mm > 4.9);
    let output = dir.path().join("out");
    let error = faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap_err();
    assert!(error.contains("box"), "{error}");
    assert!(error.contains("lid"), "{error}");
    assert!(error.contains("interference"), "{error}");
    assert!(error.contains("depth"), "{error}");
    assert!(
        error.contains("above the declared contact limit of 0.600 mm"),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn raised_limit_allows_a_five_millimetre_press() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    lid_at(&mut project, 5.);
    declare_with_limit(&mut project, Some(6.)).unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    let output = dir.path().join("out");
    faber_wiring::export_checked(
        &project.bodies,
        &project.data.contacts,
        &output,
        "stl",
        false,
    )
    .unwrap();
}

#[test]
fn strict_batch_refuses_a_move_that_exceeds_the_declared_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    declare_with_limit(&mut project, None).unwrap();
    let result = project.batch_strict(vec![Edit::PutPart {
        id: "lid".into(),
        part: boxed([10., 10., 10.], [0., 0., 5.]),
    }]);
    assert!(result.is_err());
    assert_eq!(project.data.revision, 3);
}

#[test]
fn negative_or_non_finite_limit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    assert!(declare_with_limit(&mut project, Some(-0.1)).is_err());
    assert!(declare_with_limit(&mut project, Some(f64::NAN)).is_err());
    assert!(declare_with_limit(&mut project, Some(f64::INFINITY)).is_err());
    assert!(project.data.contacts.is_empty());
}

#[test]
fn project_without_limit_field_loads_and_saves_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    declare_with_limit(&mut project, None).unwrap();
    drop(project);
    let before = std::fs::read(dir.path().join("assembly.json")).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("max_depth_mm"));
    let data = faber_files::read(dir.path()).unwrap();
    assert!((data.contacts["box|lid"].max_depth_mm - 0.6).abs() < 1e-12);
}

#[test]
fn declared_touch_is_reported_as_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = stacked_boxes(&dir);
    // A sub-micron overlap is a touch. The mesh depth is above zero, so the scan cannot allow it.
    lid_at(&mut project, 9.999_999_999);
    declare_with_limit(&mut project, Some(0.)).unwrap();
    let report = faber_wiring::interference(&project.bodies, 0.1, &project.data.contacts).unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert!(report.contacts.is_empty(), "{:?}", report.contacts);
    assert_eq!(report.allowed_contacts.len(), 1);
    let json: Value = serde_json::to_value(&report).unwrap();
    assert_eq!(json["allowed_contacts"][0]["kind"], "allowed_contact");
}
