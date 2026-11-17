use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn curved_recipes_use_parameters_and_rebuild_exact_solids() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Curves".into())).unwrap();
    project
        .set_params([("radius".into(), "3".into())].into())
        .unwrap();
    let pi = std::f64::consts::PI;
    for (geometry, expected) in [
        (json!({"op":"sphere","radius":"radius"}), 36. * pi),
        (
            json!({"op":"cone","base_radius":4,"top_radius":2,"height":9}),
            84. * pi,
        ),
        (
            json!({"op":"torus","major_radius":10,"minor_radius":2}),
            80. * pi * pi,
        ),
        (
            json!({"op":"revolve","profile":{"outer":[[2,0],[4,0],[4,10],[2,10]]},"origin":[0,0,0],"axis":[0,1,0],"angle":180}),
            60. * pi,
        ),
    ] {
        let part: Part<Scalar> = serde_json::from_value(
            json!({"name":"Curved solid","colour":[100,140,170],"geometry":geometry}),
        )
        .unwrap();
        project.put_part("shape".into(), part).unwrap();
        assert!((project.bodies[0].volume - expected).abs() < 1e-6);
    }
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 60. * pi).abs() < 1e-6);
}

#[test]
fn loft_recipe_rebuilds_when_section_height_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Loft".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Transition", "colour":[100,140,170],
        "geometry":{"op":"loft","ruled":true,"sections":[
            {"profile":{"outer":[[-5,-5],[5,-5],[5,5],[-5,5]]}},
            {"profile":{"outer":[[-10,-10],[10,-10],[10,10],[-10,10]]},"placement":{"translation":[0,0,"height"]}}
        ]}
    })).unwrap();
    project.put_part("transition".into(), part).unwrap();
    assert!((project.bodies[0].volume - 7000.).abs() < 1e-5);
    project
        .set_params([("height".into(), "60".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 14000.).abs() < 1e-5);
}

#[test]
fn sweep_recipe_rebuilds_the_path_and_refuses_zero_length_segments() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Sweep".into())).unwrap();
    project
        .set_params([("length".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Rail", "colour":[100,140,170],
        "geometry":{"op":"sweep","profile":{"outer":[[-2,-2],[2,-2],[2,2],[-2,2]]},"path":[[0,0,0],[0,0,"length"]]}
    })).unwrap();
    project.put_part("rail".into(), part).unwrap();
    assert!((project.bodies[0].volume - 480.).abs() < 1e-5);
    project
        .set_params([("length".into(), "60".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 960.).abs() < 1e-5);
    let before = project.data.clone();
    assert!(
        project
            .set_params([("length".into(), "0".into())].into())
            .is_err()
    );
    assert_eq!(project.data, before);
}
