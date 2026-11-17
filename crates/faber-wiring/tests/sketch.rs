use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn exact_circle_sketch_extrudes_and_rebuilds_after_radius_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Sketch".into())).unwrap();
    project
        .set_params([("radius".into(), "5".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Tube","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"section","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":"radius"},"holes":[{"kind":"circle","centre":[0,0],"radius":2}]}}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,0,10]}}
    ]}})).unwrap();
    project.put_part("tube".into(), part).unwrap();
    assert!((project.bodies[0].volume - 210. * std::f64::consts::PI).abs() < 1e-6);
    project
        .set_params([("radius".into(), "6".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 320. * std::f64::consts::PI).abs() < 1e-6);
    let before = project.data.clone();
    assert!(
        project
            .set_params([("radius".into(), "1".into())].into())
            .is_err()
    );
    assert_eq!(before, project.data);
}

#[test]
fn a_placed_rectangle_sketch_extrudes_in_world_coordinates() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Sketch plane".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"section","geometry":{"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[10,20]}}}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,-30,0]}}
    ]}})).unwrap();
    project.put_part("block".into(), part).unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
    assert!((project.bodies[0].bounds[0][1] + 30.).abs() < 1e-6);
    assert!((project.bodies[0].bounds[1][2] - 20.).abs() < 1e-6);
}

#[test]
fn arc_and_bezier_paths_remain_exact_closed_sketches() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Curved sketch".into())).unwrap();
    for (boundary, area) in [
        (
            json!({"kind":"path","start":[-2,0],"segments":[{"kind":"arc","via":[0,2],"end":[2,0]},{"kind":"line","end":[-2,0]}]}),
            2. * std::f64::consts::PI,
        ),
        (
            json!({"kind":"path","start":[0,0],"segments":[{"kind":"bezier","control1":[0,2],"control2":[2,2],"end":[2,0]},{"kind":"line","end":[0,0]}]}),
            2.4,
        ),
    ] {
        let part: Part<Scalar> = serde_json::from_value(json!({"name":"Curve","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
            {"id":"section","geometry":{"op":"sketch","profile":{"outer":boundary}}},
            {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,0,10]}}
        ]}})).unwrap();
        project.put_part("curve".into(), part).unwrap();
        assert!(
            (project.bodies[0].volume - area * 10.).abs() < 1e-5,
            "{}",
            project.bodies[0].volume
        );
    }
}

#[test]
fn curved_boundaries_accept_holes_and_refuse_open_paths() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Curved hole".into())).unwrap();
    let recipe = json!({"name":"Section","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"section","geometry":{"op":"sketch","profile":{"outer":{"kind":"path","start":[-2,0],"segments":[{"kind":"arc","via":[0,2],"end":[2,0]},{"kind":"line","end":[-2,0]}]},"holes":[{"kind":"circle","centre":[0,1],"radius":0.25}]}}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,0,10]}}
    ]}});
    let part: Part<Scalar> = serde_json::from_value(recipe.clone()).unwrap();
    project.put_part("section".into(), part).unwrap();
    assert!((project.bodies[0].volume - 19.375 * std::f64::consts::PI).abs() < 1e-5);
    let before = project.data.clone();
    let mut open = recipe;
    open["geometry"]["features"][0]["geometry"]["profile"]["outer"]["segments"][1]["end"] =
        json!([-1, 0]);
    let open: Part<Scalar> = serde_json::from_value(open).unwrap();
    assert!(project.put_part("section".into(), open).is_err());
    assert_eq!(before, project.data);
}

#[test]
fn ellipse_and_regular_polygon_sketches_have_known_areas() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Sketch shapes".into())).unwrap();
    for (boundary, volume) in [
        (
            json!({"kind":"ellipse","centre":[0,0],"major_radius":3,"minor_radius":2,"rotation":30}),
            60. * std::f64::consts::PI,
        ),
        (
            json!({"kind":"regular_polygon","centre":[0,0],"radius":2,"sides":6,"rotation":0}),
            60. * 3_f64.sqrt(),
        ),
    ] {
        let part: Part<Scalar> = serde_json::from_value(json!({"name":"Shape","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
            {"id":"section","geometry":{"op":"sketch","profile":{"outer":boundary}}},
            {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,0,10]}}
        ]}})).unwrap();
        project.put_part("shape".into(), part).unwrap();
        assert!(
            (project.bodies[0].volume - volume).abs() < 1e-5,
            "Volume: {}; expected {volume}",
            project.bodies[0].volume
        );
    }
}
