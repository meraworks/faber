use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn grids_translate_each_loop_kind_without_changing_its_dimensions() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Grid profiles".into())).unwrap();
    let pi = std::f64::consts::PI;
    for (item, area) in [
        (
            json!({"kind":"slot","centre":[5,5],"length":4,"width":2,"rotation":25}),
            4. + pi,
        ),
        (
            json!({"kind":"ellipse","centre":[5,5],"major_radius":2,"minor_radius":1,"rotation":25}),
            2. * pi,
        ),
        (
            json!({"kind":"regular_polygon","centre":[5,5],"radius":1,"sides":4,"rotation":25}),
            2.,
        ),
        (json!({"kind":"rectangle","origin":[4,4],"size":[2,3]}), 6.),
        (json!({"kind":"polygon","points":[[4,4],[6,4],[5,6]]}), 2.),
        (
            json!({"kind":"path","start":[4,4],"segments":[{"kind":"spline","points":[[5,4],[6,4]],"tangents":[[1,0],[1,0]]},{"kind":"line","end":[6,6]},{"kind":"line","end":[4,6]},{"kind":"line","end":[4,4]}]}),
            4.,
        ),
        (
            json!({"kind":"path","start":[4,5],"segments":[{"kind":"arc","via":[5,6],"end":[6,5]},{"kind":"line","end":[4,5]}]}),
            pi / 2.,
        ),
        (
            json!({"kind":"path","start":[4,4],"segments":[{"kind":"bezier","control1":[4,5],"control2":[6,5],"end":[6,4]},{"kind":"line","end":[4,4]}]}),
            1.2,
        ),
    ] {
        let recipe = json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[30,30]},"holes":[{"kind":"grid","item":item,"count":[2,2],"step":[[10,1],[-1,10]]}]}}},
            {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,1]}}
        ]}});
        project
            .put_part(
                "plate".into(),
                serde_json::from_value::<Part<Scalar>>(recipe).unwrap(),
            )
            .unwrap();
        assert!(
            (project.bodies[0].volume - (900. - 4. * area)).abs() < 1e-5,
            "Item: {item}; volume: {}",
            project.bodies[0].volume
        );
    }
}

#[test]
fn sketch_grid_repeats_holes_and_rebuilds_from_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Sketch array".into())).unwrap();
    project
        .set_params([("pitch".into(), "10".into()), ("radius".into(), "1".into())].into())
        .unwrap();
    let recipe = json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[30,40]},"holes":[
            {"kind":"grid","item":{"kind":"circle","centre":[5,5],"radius":"radius"},"count":[2,3],"step":[["pitch",0],[0,"pitch"]]}
        ]}}},
        {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,2]}}
    ]}});
    project
        .put_part(
            "plate".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    assert!((project.bodies[0].volume - (2400. - 12. * std::f64::consts::PI)).abs() < 1e-5);
    project
        .set_params([("pitch".into(), "12".into()), ("radius".into(), "2".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - (2400. - 48. * std::f64::consts::PI)).abs() < 1e-5);
    let (_, faces) = project.feature_topology("plate", "solid").unwrap();
    assert_eq!(faces.len(), 12);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for count in [json!([0, 3]), json!([257, 1]), json!([1000000, 1000000])] {
        let mut invalid = recipe.clone();
        invalid["geometry"]["features"][0]["geometry"]["profile"]["holes"][0]["count"] = count;
        assert!(
            project
                .put_part(
                    "plate".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
    assert!(
        project
            .set_params([("pitch".into(), "0".into())].into())
            .is_err()
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - (2400. - 48. * std::f64::consts::PI)).abs() < 1e-5);
}
