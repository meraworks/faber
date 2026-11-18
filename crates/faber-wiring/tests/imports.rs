use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn purchased_screw_can_move_but_its_geometry_is_fixed() {
    let root = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(root.path(), true, Some("Assembly".into())).unwrap();
    let hash = "f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e";
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fastener-check/assets")
        .join(hash);
    project
        .import_with_record(
            "screw".into(),
            "Screw".into(),
            &source,
            true,
            Some(faber_model::Purchased {
                source: "FreeCAD-library".into(),
                licence: "CC BY 3.0".into(),
                designation: "M3 x 10".into(),
                mass_g: None,
            }),
        )
        .unwrap();
    let mut part = faber_wiring::read(root.path()).unwrap().0.parts["screw"].clone();
    part.placement.translation[0] = Scalar::Number(12.);
    project.put_part("screw".into(), part.clone()).unwrap();
    part.geometry = serde_json::from_value(json!({"op":"box","size":[3,3,10]})).unwrap();
    let before = std::fs::read(root.path().join("faber.json")).unwrap();
    assert!(project.put_part("screw".into(), part).is_err());
    assert_eq!(
        before,
        std::fs::read(root.path().join("faber.json")).unwrap()
    );
}

#[test]
fn imported_mesh_keeps_faces_and_reopens_without_source() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("mesh");
    let source = directory.path().join("tetra.stl");
    let mut bytes = vec![0u8; 80];
    bytes.extend(4u32.to_le_bytes());
    let vertices = [[0f32, 0., 0.], [2., 0., 0.], [0., 3., 0.], [0., 0., 4.]];
    for face in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        bytes.extend([0u8; 12]);
        for index in face {
            for value in vertices[index] {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.extend([0u8; 2]);
    }
    std::fs::write(&source, &bytes).unwrap();
    let mut project = faber_wiring::open(&root, true, Some("Mesh".into())).unwrap();
    project
        .import_part("plate".into(), "Plate".into(), &source, false)
        .unwrap();
    assert_eq!(project.bodies[0].mesh.triangles.len(), 4);
    assert_eq!(project.bodies[0].bounds, [[0., 0., 0.], [2., 3., 4.]]);
    assert!((project.bodies[0].volume - 4.).abs() < 1e-9);
    assert!(project.bodies[0].shape.is_none());
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    std::fs::remove_file(&source).unwrap();
    let (_, bodies) = faber_wiring::read(&root).unwrap();
    assert_eq!(bodies[0].mesh.triangles.len(), 4);
    project.undo().unwrap();
    assert!(project.bodies.is_empty());
    project.redo().unwrap();
    assert_eq!(project.bodies[0].mesh.triangles.len(), 4);
    let mut moved = project.data.parts["plate"].clone();
    moved.placement.translation = [10., 20., 30.].map(Scalar::Number);
    project.put_part("plate".into(), moved).unwrap();
    assert_eq!(project.bodies[0].bounds, [[10., 20., 30.], [12., 23., 34.]]);
    let (_, reopened) = faber_wiring::read(&root).unwrap();
    assert_eq!(reopened[0].bounds, project.bodies[0].bounds);
    assert!(
        project
            .topology("plate")
            .unwrap_err()
            .contains("no exact shape")
    );
}

#[test]
fn imported_solid_reopens_without_its_source_and_detects_asset_changes() {
    let directory = tempfile::tempdir().unwrap();
    let original_root = directory.path().join("original");
    let mut original = faber_wiring::open(&original_root, true, Some("Original".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(
        json!({"name":"Block","colour":[80,100,120],"geometry":{"op":"box","size":[10,20,30]}}),
    )
    .unwrap();
    original.put_part("block".into(), part).unwrap();
    let output = directory.path().join("output");
    faber_wiring::export_checked(
        &original.bodies,
        &original.data.contacts,
        &output,
        "step",
        false,
    )
    .unwrap();
    let root = directory.path().join("imported");
    let mut project = faber_wiring::open(&root, true, Some("Imported".into())).unwrap();
    project
        .import_part(
            "block".into(),
            "Imported block".into(),
            &output.join("block.step"),
            false,
        )
        .unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-5);
    std::fs::remove_file(output.join("block.step")).unwrap();
    let (_, bodies) = faber_wiring::read(&root).unwrap();
    assert!((bodies[0].volume - 6000.).abs() < 1e-5);
    let imported = project.data.clone();
    project.undo().unwrap();
    assert!(project.bodies.is_empty());
    project.redo().unwrap();
    assert_eq!(project.data.parts, imported.parts);
    let geometry = serde_json::to_value(&project.data.parts["block"].geometry).unwrap();
    let edited: Part<Scalar> = serde_json::from_value(json!({"name":"Imported row","colour":[80,100,120],"geometry":{"op":"history","output":"row","features":[
        {"id":"imported","geometry":geometry},
        {"id":"row","geometry":{"op":"linear_pattern","source":"imported","count":2,"step":[30,0,0]}}
    ]}})).unwrap();
    project.put_part("block".into(), edited).unwrap();
    assert!((project.bodies[0].volume - 12000.).abs() < 1e-5);
    let (_, bodies) = faber_wiring::read(&root).unwrap();
    assert!((bodies[0].volume - 12000.).abs() < 1e-5);
    let saved = std::fs::read(root.join("faber.json")).unwrap();
    let asset = std::fs::read_dir(root.join("assets"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(asset, b"changed").unwrap();
    assert!(faber_wiring::read(&root).is_err());
    assert!(
        project
            .set_params([("unused".into(), "1".into())].into())
            .is_err()
    );
    assert_eq!(std::fs::read(root.join("faber.json")).unwrap(), saved);
}

#[test]
fn malformed_import_keeps_the_saved_project_and_existing_parts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let mut project = faber_wiring::open(&root, true, Some("Invalid import".into())).unwrap();
    let saved = std::fs::read(root.join("faber.json")).unwrap();
    let source = directory.path().join("bad.step");
    std::fs::write(&source, b"not a model").unwrap();
    assert!(
        project
            .import_part("part".into(), "Part".into(), &source, false)
            .is_err()
    );
    assert!(project.bodies.is_empty());
    assert_eq!(std::fs::read(root.join("faber.json")).unwrap(), saved);
}
#[test]
fn exported_sphere_facets_follow_the_exact_surface() {
    let root = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(root.path(), true, Some("Sphere".into())).unwrap();
    project
        .put_part(
            "ball".into(),
            serde_json::from_value::<Part<Scalar>>(
                json!({"name":"Ball","colour":[80,100,120],"geometry":{"op":"sphere","radius":16}}),
            )
            .unwrap(),
        )
        .unwrap();
    let out = root.path().join("out");
    faber_wiring::export(&project.bodies, &out).unwrap();
    let data = std::fs::read(out.join("ball.stl")).unwrap();
    let mut worst = 0.0_f64;
    for face in data[84..].as_chunks::<50>().0 {
        let mut centre = [0.0; 3];
        for vertex in face[12..48].as_chunks::<12>().0 {
            for (axis, bytes) in vertex.as_chunks::<4>().0.iter().enumerate() {
                centre[axis] += f32::from_le_bytes(*bytes) as f64 / 3.0;
            }
        }
        worst = worst.max(16.0 - centre.iter().map(|x| x * x).sum::<f64>().sqrt());
    }
    assert!(worst < 0.003, "Facet centre deviation: {worst} mm");
}
