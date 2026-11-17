use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::json;

#[test]
fn sketch_tangent_expressions_rebuild_and_refuse_invalid_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Sketch tangents".into())).unwrap();
    project
        .set_params([("slope".into(), "2".into())].into())
        .unwrap();
    let recipe = json!({"name":"Arch","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"path","start":[0,0],"segments":[
            {"kind":"spline","points":[[5,5],[10,0]],"tangents":[[1,"slope"],[1,"-slope"]]},
            {"kind":"line","end":[0,0]}]}}}},
        {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,10]}}
    ]}});
    project
        .put_part(
            "arch".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    let first = project.bodies[0].volume;
    project
        .set_params([("slope".into(), "0".into())].into())
        .unwrap();
    let second = project.bodies[0].volume;
    assert!(first > 0. && second > 0. && (first - second).abs() > 1.);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for tangents in [
        json!([[0, 0], [1, 0]]),
        json!([[1, 0], [0, 0]]),
        json!([[1, "missing"], [1, 0]]),
    ] {
        let mut invalid = recipe.clone();
        invalid["geometry"]["features"][0]["geometry"]["profile"]["outer"]["segments"][0]["tangents"] =
            tangents;
        assert!(
            project
                .put_part(
                    "arch".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - second).abs() < 1e-5);
}

#[test]
fn spline_end_tangents_control_both_ends() {
    let kernel = faber_kernel_occt::OcctKernel;
    let recipe = json!({"start":[0,0,0],"segments":[{"kind":"spline","points":[[5,0,10],[0,0,20]],"tangents":[[0,0,1],[0,0,1]]}]});
    let path = serde_json::from_value(recipe.clone()).unwrap();
    let curve = kernel.curve(&path).unwrap();
    for z in [0.001, 19.999] {
        let probe = serde_json::from_value(
            json!({"start":[0,0,z],"segments":[{"kind":"line","end":[0,0.000001,z]}]}),
        )
        .unwrap();
        let distance = kernel
            .distance(&curve, &kernel.curve(&probe).unwrap())
            .unwrap()
            .distance_mm;
        assert!(distance < 1e-6, "Tangent distance: {distance}");
    }
    for tangents in [
        json!([[0, 0, 0], [0, 0, 1]]),
        json!([[0, 0, 1], [0, 0, 0]]),
        json!([[0, 0, 1000001], [0, 0, 1]]),
    ] {
        let mut invalid = recipe.clone();
        invalid["segments"][0]["tangents"] = tangents;
        assert!(
            kernel
                .curve(&serde_json::from_value(invalid).unwrap())
                .is_err()
        );
    }
}

#[test]
fn spline_sketch_extrudes_and_spline_path_sweeps_with_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Spline".into())).unwrap();
    project
        .set_params([("length".into(), "20".into())].into())
        .unwrap();
    let sketch = json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"path","start":[0,0],"segments":[
            {"kind":"spline","points":[[5,0],[10,0]]},{"kind":"line","end":[10,10]},
            {"kind":"line","end":[0,10]},{"kind":"line","end":[0,0]}]}}}},
        {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,"length"]}}
    ]}});
    project
        .put_part(
            "plate".into(),
            serde_json::from_value::<Part<Scalar>>(sketch).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - 2000.).abs() < 1e-5);
    let sweep = json!({"name":"Rod","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":1}}}},
        {"id":"path","geometry":{"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"spline","points":[[0,0,10],[0,0,"length"]]}]}}},
        {"id":"solid","geometry":{"op":"sweep_sketch","source":"profile","path":"path"}}
    ]}});
    project
        .put_part(
            "rod".into(),
            serde_json::from_value::<Part<Scalar>>(sweep.clone()).unwrap(),
        )
        .unwrap();
    project
        .set_params([("length".into(), "30".into())].into())
        .unwrap();
    let volume = project
        .bodies
        .iter()
        .find(|b| b.id == "rod")
        .unwrap()
        .volume;
    assert!(
        (volume - 30. * std::f64::consts::PI).abs() < 1e-5,
        "Volume: {volume}; expected {}",
        30. * std::f64::consts::PI
    );
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for points in [
        json!([]),
        json!([[0, 0, 10]]),
        json!([[0, 0, 0], [0, 0, 10]]),
        json!([[0, 0, 10], [0, 0, 10]]),
        json!([[0, 0, 10], [0, 0, 1000001]]),
    ] {
        let mut invalid = sweep.clone();
        invalid["geometry"]["features"][1]["geometry"]["path"]["segments"][0]["points"] = points;
        assert!(
            project
                .put_part(
                    "rod".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies.iter().find(|b| b.id == "plate").unwrap().volume - 3000.).abs() < 1e-5);
}

#[test]
fn spatial_spline_is_one_smooth_edge_through_each_point() {
    let kernel = faber_kernel_occt::OcctKernel;
    let points = json!([[5, 4, 3], [10, -2, 8], [15, 0, 12]]);
    let path = serde_json::from_value(
        json!({"start":[0,0,0],"segments":[{"kind":"spline","points":points}]}),
    )
    .unwrap();
    let curve = kernel.curve(&path).unwrap();
    assert_eq!(kernel.properties(&curve).unwrap().edges, 1);
    for point in points.as_array().unwrap() {
        let p: [f64; 3] = serde_json::from_value(point.clone()).unwrap();
        let probe = serde_json::from_value(
            json!({"start":p,"segments":[{"kind":"line","end":[p[0],p[1],p[2]+0.01]}]}),
        )
        .unwrap();
        assert!(
            kernel
                .distance(&curve, &kernel.curve(&probe).unwrap())
                .unwrap()
                .distance_mm
                < 1e-7
        );
    }
}
