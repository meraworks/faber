use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::json;

#[test]
fn periodic_sketch_closes_without_a_line_and_scales_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Periodic spline".into())).unwrap();
    project
        .set_params([("r".into(), "10".into())].into())
        .unwrap();
    let recipe = json!({"name":"Disk","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"path","start":["r",0],"segments":[{"kind":"spline","periodic":true,"points":[[0,"r"],["-r",0],[0,"-r"]]}]}}}},
        {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,2]}}
    ]}});
    project
        .put_part(
            "disk".into(),
            serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
        )
        .unwrap();
    let volume = project.bodies[0].volume;
    assert!(volume > 400. && volume < 800.);
    let (_, faces) = project.feature_topology("disk", "solid").unwrap();
    assert_eq!(faces.len(), 3);
    project
        .set_params([("r".into(), "20".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 4. * volume).abs() < 1e-4);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for points in [json!([[0, 10]]), json!([[0, 10], [-10, 0], [20, 0]])] {
        let mut invalid = recipe.clone();
        invalid["geometry"]["features"][0]["geometry"]["profile"]["outer"]["segments"][0]["points"] =
            points;
        assert!(
            project
                .put_part(
                    "disk".into(),
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
    assert!((bodies[0].volume - 4. * volume).abs() < 1e-4);
}

#[test]
fn periodic_spatial_curve_has_one_closed_edge_through_its_points() {
    let kernel = faber_kernel_occt::OcctKernel;
    let path=serde_json::from_value(json!({"start":[10,0,0],"segments":[{"kind":"spline","periodic":true,"points":[[0,10,2],[-10,0,0],[0,-10,-2]]}]})).unwrap();
    let curve = kernel.curve(&path).unwrap();
    assert_eq!(kernel.properties(&curve).unwrap().edges, 1);
    for p in [
        [10., 0., 0.],
        [0., 10., 2.],
        [-10., 0., 0.],
        [0., -10., -2.],
    ] {
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
    let edges = kernel.edges(&curve).unwrap();
    assert_eq!(edges.len(), 1);
    assert!(edges[0].length > 60. && edges[0].length < 70.);
    let mut directions = Vec::new();
    for y in [-0.001, 0.001] {
        let probe = serde_json::from_value(
            json!({"start":[9.99,y,0.2*y],"segments":[{"kind":"line","end":[10.01,y,0.2*y]}]}),
        )
        .unwrap();
        let measured = kernel
            .distance(&curve, &kernel.curve(&probe).unwrap())
            .unwrap();
        assert!(measured.distance_mm < 1e-7);
        let vector = [
            (measured.point_a[0] - 10.) / y,
            measured.point_a[1] / y,
            measured.point_a[2] / y,
        ];
        let length = vector.iter().map(|v| v * v).sum::<f64>().sqrt();
        directions.push(vector.map(|v| v / length));
    }
    let dot = directions[0]
        .iter()
        .zip(directions[1])
        .map(|(a, b)| a * b)
        .sum::<f64>();
    assert!(dot > 0.999999, "Seam tangent dot product: {dot}");
    let mut invalid = serde_json::to_value(&path).unwrap();
    invalid["segments"][0]["tangents"] = json!([[0, 1, 0], [0, 1, 0]]);
    assert!(
        kernel
            .curve(&serde_json::from_value(invalid).unwrap())
            .is_err()
    );
    let mut invalid = serde_json::to_value(&path).unwrap();
    invalid["segments"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"line","end":[20,0,0]}));
    assert!(
        kernel
            .curve(&serde_json::from_value(invalid).unwrap())
            .is_err()
    );
}
