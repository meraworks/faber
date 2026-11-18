use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn curved_distance_uses_the_exact_surfaces() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Curved measure".into())).unwrap();
    for (id, radius, translation) in [("a", 2., [0., 0., 0.]), ("b", 3., [3., 4., 12.])] {
        let part: Part<Scalar> = serde_json::from_value(json!({"name":id,"colour":[100,140,170],
            "geometry":{"op":"sphere","radius":radius},"placement":{"translation":translation,"rotation":[20,30,40]}
        })).unwrap();
        project.put_part(id.into(), part).unwrap();
    }
    let measured = project.measure_distance("a", "b").unwrap();
    assert!((measured.distance_mm - 8.).abs() < 1e-7);
    for (actual, expected) in measured
        .point_a
        .into_iter()
        .zip([6. / 13., 8. / 13., 24. / 13.])
    {
        assert!((actual - expected).abs() < 1e-7);
    }
    for (actual, expected) in measured
        .point_b
        .into_iter()
        .zip([30. / 13., 40. / 13., 120. / 13.])
    {
        assert!((actual - expected).abs() < 1e-7);
    }
}

#[test]
fn minimum_distance_measures_placed_solids_without_saving() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Measure".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(
        json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}}),
    )
    .unwrap();
    project.put_part("a".into(), part.clone()).unwrap();
    let mut other = part;
    other.placement.translation = [13., 24., 0.].map(Scalar::Number);
    project.put_part("b".into(), other.clone()).unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let measured = project.measure_distance("a", "b").unwrap();
    assert!((measured.distance_mm - 5.).abs() < 1e-7);
    assert!((measured.point_a[0] - 10.).abs() < 1e-7);
    assert!((measured.point_a[1] - 20.).abs() < 1e-7);
    assert!((measured.point_b[0] - 13.).abs() < 1e-7);
    assert!((measured.point_b[1] - 24.).abs() < 1e-7);
    assert!((project.measure_distance("b", "a").unwrap().distance_mm - 5.).abs() < 1e-7);
    assert!(project.measure_distance("a", "missing").is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    for x in [10., 5., 0.] {
        other.placement.translation = [x, 0., 0.].map(Scalar::Number);
        project.put_part("b".into(), other.clone()).unwrap();
        assert!(project.measure_distance("a", "b").unwrap().distance_mm < 1e-7);
    }
    other.geometry = faber_model::Geometry::Box {
        size: [2., 2., 2.].map(Scalar::Number),
    };
    other.placement.translation = [1., 1., 1.].map(Scalar::Number);
    project.put_part("b".into(), other).unwrap();
    assert!(project.measure_distance("a", "b").unwrap().distance_mm < 1e-7);
}
