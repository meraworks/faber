use faber_core::Project;
use faber_model::{Edit, Feature, Geometry, Part, Scalar};
use serde_json::json;
use std::path::{Path, PathBuf};

fn exact() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/round-surfaces/out/block.step")
}
fn feature() -> Feature<Scalar> {
    serde_json::from_value(json!({"id":"lift","geometry":{"op":"move_faces","body":"source","faces":[{"normal":[0.,0.,1.],"tolerance":0.0001,"expect":1}],"distance":2.}})).unwrap()
}
fn wrapped(p: &Project, id: &str) -> Part<Scalar> {
    let mut part = p.data.parts[id].clone();
    part.geometry = Geometry::History {
        features: vec![
            Feature {
                id: "source".into(),
                geometry: part.geometry,
                suppressed: false,
            },
            feature(),
        ],
        output: "lift".into(),
    };
    part
}
#[test]
fn first_face_feature_on_an_import_keeps_its_asset_and_adds_one_undo_step() {
    let root = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(root.path(), "Part".into()).unwrap();
    p.import_part("plate".into(), "Plate".into(), &exact(), false)
        .unwrap();
    let before = p.data.clone();
    let top = p.bodies[0].bounds[1][2];
    p.add_feature("plate", feature(), None, true).unwrap();
    let Geometry::History { features, output } = &p.data.parts["plate"].geometry else {
        panic!()
    };
    assert_eq!(features[0].geometry, before.parts["plate"].geometry);
    assert_eq!(features[0].id, "source");
    assert_eq!(output, "lift");
    assert!((p.bodies[0].bounds[1][2] - top - 2.).abs() < 1e-5);
    assert_eq!(p.data.revision, before.revision + 1);
    p.undo().unwrap();
    assert_eq!(p.data.parts, before.parts);
}
fn refusal(p: &mut Project, root: &Path, id: &str, fragment: &str) {
    let old = p.data.clone();
    let fingerprint = faber_wiring::project_fingerprint(root).unwrap();
    let err = p.add_feature(id, feature(), None, true).unwrap_err();
    assert!(err.contains(fragment), "{err}");
    let err = p.put_part(id.into(), wrapped(p, id)).unwrap_err();
    assert!(
        err.to_lowercase().contains(&fragment.to_lowercase()),
        "{err}"
    );
    assert_eq!(p.data, old);
    assert_eq!(
        faber_wiring::project_fingerprint(root).unwrap(),
        fingerprint
    );
}
#[test]
fn mesh_only_and_protected_occurrences_keep_their_revision_on_a_face_edit() {
    let root = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(root.path(), "Parts".into()).unwrap();
    let mesh = root.path().join("flat.off");
    std::fs::write(&mesh, "OFF\n3 1 0\n0 0 0\n10 0 0\n0 10 0\n3 0 1 2\n").unwrap();
    p.import_part("mesh".into(), "Mesh".into(), &mesh, false)
        .unwrap();
    refusal(
        &mut p,
        root.path(),
        "mesh",
        "Part mesh is a mesh. Convert it to a solid first.",
    );
    let screw=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fastener-check/assets/f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e");
    let record = faber_model::Purchased {
        designation: "M3 x 10".into(),
        source: "FreeCAD-library".into(),
        licence: "CC BY 3.0".into(),
        mass_g: None,
    };
    p.import_with_record("screw".into(), "Screw".into(), &screw, false, Some(record))
        .unwrap();
    refusal(&mut p, root.path(), "screw", "purchased");
    p.move_linked(
        "screw",
        &faber_core::Motion::Translate([4., 0., 0.]),
        "spare",
    )
    .unwrap();
    refusal(
        &mut p,
        root.path(),
        "spare",
        "Part spare is an instance of screw",
    );
    faber_wiring::links::import(
        root.path(),
        &mut p,
        "linked".into(),
        "Linked".into(),
        &exact(),
        false,
    )
    .unwrap();
    refusal(&mut p, root.path(), "linked", "follows the file");

    let forged = wrapped(&p, "screw");
    let revision = p.data.revision;
    assert!(
        p.batch(vec![Edit::PutPart {
            id: "copy".into(),
            part: forged
        }])
        .unwrap_err()
        .to_lowercase()
        .contains("purchased")
    );
    assert_eq!(p.data.revision, revision);
}
