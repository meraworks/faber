use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn successive_sketch_placements_keep_the_plane_and_hole_for_extrusion() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Placed sketch".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Tube","colour":[100,140,170],"geometry":{"op":"history","output":"tube","features":[
        {"id":"section","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":4},"holes":[{"kind":"circle","centre":[0,0],"radius":2}]}}},
        {"id":"turned","geometry":{"op":"transform","source":"section","placement":{"rotation":[90,0,0]}}},
        {"id":"moved","geometry":{"op":"transform","source":"turned","placement":{"translation":[10,20,30]}}},
        {"id":"tube","geometry":{"op":"extrude_sketch","source":"moved","vector":[0,15,0]}}
    ]}})).unwrap();
    project.put_part("tube".into(), part).unwrap();
    assert!((project.bodies[0].volume - 180. * std::f64::consts::PI).abs() < 1e-5);
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip([6., 20., 26., 14., 35., 34.])
    {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
}

#[test]
fn transformed_sketch_and_curve_build_a_solid_and_rebuild_after_parameter_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Moved profiles".into())).unwrap();
    project
        .set_params([("offset".into(), "10".into())].into())
        .unwrap();
    let recipe = json!({"name":"Rod","colour":[100,140,170],"geometry":{"op":"history","output":"rod","features":[
        {"id":"section","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":2}}}},
        {"id":"moved_section","geometry":{"op":"transform","source":"section","placement":{"translation":["offset",0,0]}}},
        {"id":"path","geometry":{"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[0,0,30]}]}}},
        {"id":"moved_path","geometry":{"op":"transform","source":"path","placement":{"translation":["offset",0,0]}}},
        {"id":"rod","geometry":{"op":"sweep_sketch","source":"moved_section","path":"moved_path"}}
    ]}});
    let part: Part<Scalar> = serde_json::from_value(recipe.clone()).unwrap();
    project.put_part("rod".into(), part).unwrap();
    assert!((project.bodies[0].volume - 120. * std::f64::consts::PI).abs() < 1e-5);
    assert!((project.bodies[0].bounds[0][0] - 8.).abs() < 1e-5);
    project
        .set_params([("offset".into(), "25".into())].into())
        .unwrap();
    assert!((project.bodies[0].bounds[0][0] - 23.).abs() < 1e-5);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].bounds[0][0] - 23.).abs() < 1e-5);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for output in ["moved_section", "moved_path"] {
        let mut invalid = recipe.clone();
        invalid["geometry"]["output"] = json!(output);
        let part: Part<Scalar> = serde_json::from_value(invalid).unwrap();
        assert!(project.put_part("rod".into(), part).is_err());
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
}
