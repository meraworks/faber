use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::json;

fn part(keep: &str, origin: serde_json::Value, normal: serde_json::Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Split block","colour":[100,140,170],
        "geometry":{"op":"history","output":"cut","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
            {"id":"cut","geometry":{"op":"split","source":"base","origin":origin,"normal":normal,"keep":keep}}
        ]}
    })).unwrap()
}

#[test]
fn split_keeps_both_or_one_side_and_rebuilds_from_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Split".into())).unwrap();
    project
        .set_params([("height".into(), "12".into())].into())
        .unwrap();
    for (keep, volume, bounds) in [
        ("both", 6000., [[0., 0., 0.], [10., 20., 30.]]),
        ("positive", 3600., [[0., 0., 12.], [10., 20., 30.]]),
        ("negative", 2400., [[0., 0., 0.], [10., 20., 12.]]),
    ] {
        project
            .put_part(
                "block".into(),
                part(keep, json!([0, 0, "height"]), json!([0, 0, 2])),
            )
            .unwrap();
        assert!((project.bodies[0].volume - volume).abs() < 1e-5);
        for (actual, expected) in project.bodies[0]
            .bounds
            .iter()
            .flatten()
            .zip(bounds.iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-5);
        }
        let topology = project.feature_topology("block", "cut").unwrap();
        // The two solids share the face on the cut plane.
        assert_eq!(topology.1.len(), if keep == "both" { 11 } else { 6 });
        let info = faber_kernel_occt::OcctKernel
            .properties(project.bodies[0].shape.as_ref().unwrap())
            .unwrap();
        assert_eq!(info.solids, if keep == "both" { 2 } else { 1 });
    }
    project
        .set_params([("height".into(), "15".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 3000.).abs() < 1e-5);
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    for (origin, normal) in [
        (json!([0, 0, 40]), json!([0, 0, 1])),
        (json!([0, 0, 0]), json!([0, 0, 1])),
        (json!([0, 0, 15]), json!([0, 0, 0])),
    ] {
        assert!(
            project
                .put_part("block".into(), part("both", origin, normal))
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
    }
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 3000.).abs() < 1e-5);
}

#[test]
fn oblique_split_and_reversed_normal_select_complementary_solids() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Oblique split".into())).unwrap();
    // x + y = 10 cuts a triangle of area 50 through the 30 mm height.
    for (normal, volume) in [(json!([1, 1, 0]), 4500.), (json!([-1, -1, 0]), 1500.)] {
        project
            .put_part("block".into(), part("positive", json!([10, 0, 0]), normal))
            .unwrap();
        assert!((project.bodies[0].volume - volume).abs() < 1e-5);
    }
}
