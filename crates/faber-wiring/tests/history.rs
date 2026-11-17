use faber_model::{Part, Scalar};
use serde_json::json;

fn recipe() -> Part<Scalar> {
    serde_json::from_value(json!({
        "name":"Cut block", "colour":[120,150,170],
        "geometry":{"op":"history","output":"cut","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,"height"]}},
            {"id":"tool","geometry":{"op":"box","size":[10,20,"height"]}},
            {"id":"moved","geometry":{"op":"transform","source":"tool","placement":{"translation":[5,0,0]}}},
            {"id":"cut","geometry":{"op":"difference","left":"base","right":"moved"}}
        ]}
    })).unwrap()
}

#[test]
fn feature_topology_uses_source_coordinates_and_updates_after_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Queries".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let mut part = recipe();
    part.placement.translation = [100., 200., 300.].map(Scalar::Number);
    project.put_part("block".into(), part).unwrap();
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    let (edges, faces) = project.feature_topology("block", "base").unwrap();
    assert_eq!(edges.len(), 12);
    assert_eq!(faces.len(), 6);
    assert!(
        faces
            .iter()
            .all(|face| face.centre[0] <= 10. && face.centre[2] <= 30.)
    );
    assert!(edges.iter().any(|edge| (edge.length - 30.).abs() < 1e-8));
    let (_, placed) = project.topology("block").unwrap();
    assert!(placed.iter().all(|face| face.centre[0] >= 100.));
    assert!(project.feature_topology("block", "missing").is_err());
    assert!(project.feature_topology("missing", "base").is_err());
    assert_eq!(
        std::fs::read(directory.path().join("faber.json")).unwrap(),
        before
    );
    project
        .set_params([("height".into(), "40".into())].into())
        .unwrap();
    let (edges, _) = project.feature_topology("block", "base").unwrap();
    assert!(edges.iter().any(|edge| (edge.length - 40.).abs() < 1e-8));
    assert!(!edges.iter().any(|edge| (edge.length - 30.).abs() < 1e-8));
    drop(project);
    let mut reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(
        reopened.feature_topology("block", "base").unwrap().0.len(),
        12
    );
    let mut plain = recipe();
    plain.geometry = faber_model::Geometry::Box {
        size: [10., 20., 30.].map(Scalar::Number),
    };
    reopened.put_part("block".into(), plain).unwrap();
    assert!(reopened.feature_topology("block", "base").is_err());
}

#[test]
fn ordered_features_rebuild_after_parameter_edits_and_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("History".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    assert!((project.bodies[0].volume - 3000.).abs() < 1e-6);
    project
        .set_params([("height".into(), "40".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 4000.).abs() < 1e-6);
    drop(project);
    let reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert!((reopened.bodies[0].volume - 4000.).abs() < 1e-6);
}

#[test]
fn invalid_feature_references_and_duplicate_ids_do_not_change_saved_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("History".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for mode in 0..3 {
        let mut bad = serde_json::to_value(recipe()).unwrap();
        match mode {
            0 => bad["geometry"]["features"][0]["id"] = json!("tool"),
            1 => bad["geometry"]["features"][2]["geometry"]["source"] = json!("cut"),
            _ => bad["geometry"]["output"] = json!("absent"),
        }
        let bad: Part<Scalar> = serde_json::from_value(bad).unwrap();
        assert!(project.put_part("block".into(), bad).is_err());
        assert_eq!(
            before,
            std::fs::read(directory.path().join("faber.json")).unwrap()
        );
    }
}

#[test]
fn feature_edits_preserve_order_and_refuse_broken_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Edit".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    let changed = serde_json::from_value(
        json!({"op":"transform","source":"tool","placement":{"translation":[2,0,0]}}),
    )
    .unwrap();
    project.edit_feature("block", "moved", changed).unwrap();
    assert!((project.bodies[0].volume - 1200.).abs() < 1e-6);
    let unused = serde_json::from_value(
        json!({"id":"spare","geometry":{"op":"cylinder","radius":2,"height":10}}),
    )
    .unwrap();
    project
        .add_feature("block", unused, Some("base"), false)
        .unwrap();
    project.move_feature("block", "spare", None).unwrap();
    project.delete_feature("block", "spare").unwrap();
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    assert!(project.delete_feature("block", "tool").is_err());
    assert!(project.move_feature("block", "cut", Some("base")).is_err());
    assert!(project.delete_feature("block", "missing").is_err());
    assert_eq!(
        before,
        std::fs::read(directory.path().join("faber.json")).unwrap()
    );
    let reopened = faber_wiring::read(directory.path()).unwrap();
    assert!((reopened.1[0].volume - 1200.).abs() < 1e-6);
}
