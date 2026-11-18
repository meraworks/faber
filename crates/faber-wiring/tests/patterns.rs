use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn circular_pattern_uses_an_offset_oblique_axis_and_parameter_angles() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Circular pattern".into())).unwrap();
    project
        .set_params([("angle".into(), "120".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Balls","colour":[100,140,170],"geometry":{"op":"history","output":"ring","features":[
        {"id":"ball","geometry":{"op":"sphere","radius":1}},
        {"id":"seed","geometry":{"op":"transform","source":"ball","placement":{"translation":[110,200,300]}}},
        {"id":"ring","geometry":{"op":"circular_pattern","source":"seed","count":3,"origin":[100,200,300],"axis":[1,1,1],"angle":"angle"}}
    ]}})).unwrap();
    project.put_part("balls".into(), part).unwrap();
    assert!((project.bodies[0].volume - 4. * std::f64::consts::PI).abs() < 1e-5);
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip([99., 199., 299., 111., 211., 311.])
    {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    project
        .set_params([("angle".into(), "-120".into())].into())
        .unwrap();
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 4. * std::f64::consts::PI).abs() < 1e-5);
    let saved = faber_wiring::project_fingerprint(directory.path()).unwrap();
    assert!(
        project
            .set_params([("angle".into(), "0".into())].into())
            .is_err()
    );
    let invalid = serde_json::from_value(json!({"op":"circular_pattern","source":"seed","count":3,"origin":[100,200,300],"axis":[0,0,0],"angle":120})).unwrap();
    assert!(project.edit_feature("balls", "ring", invalid).is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        saved
    );
}

#[test]
fn circular_pattern_full_turn_spaces_copies_at_angle_over_count() {
    for (angle, label) in [("360", "positive"), ("-360", "negative")] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Full turn".into())).unwrap();
        project
            .set_params([("angle".into(), angle.into())].into())
            .unwrap();
        let part: Part<Scalar> = serde_json::from_value(json!({"name":"Pins","colour":[100,140,170],"geometry":{"op":"history","output":"ring","features":[
            {"id":"pin","geometry":{"op":"cylinder","radius":5,"height":5.5}},
            {"id":"seed","geometry":{"op":"transform","source":"pin","placement":{"translation":[115,0,-5]}}},
            {"id":"ring","geometry":{"op":"circular_pattern","source":"seed","count":6,"origin":[0,0,0],"axis":[0,0,1],"angle":"angle"}}
        ]}})).unwrap();
        project.put_part("pins".into(), part).unwrap();
        let expected = 6. * std::f64::consts::PI * 25. * 5.5;
        assert!(
            (project.bodies[0].volume - expected).abs() < 1e-3,
            "{label}: {} != {expected}",
            project.bodies[0].volume
        );
        assert!(
            project
                .set_params([("angle".into(), "361".into())].into())
                .is_err()
        );
    }
}

#[test]
fn linear_pattern_rebuilds_and_refuses_invalid_counts_without_saving() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Pattern".into())).unwrap();
    project
        .set_params([("copies".into(), "3".into()), ("pitch".into(), "20".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Blocks","colour":[100,140,170],"geometry":{"op":"history","output":"row","features":[
        {"id":"block","geometry":{"op":"box","size":[10,10,10]}},
        {"id":"row","geometry":{"op":"linear_pattern","source":"block","count":"copies","step":["pitch",0,0]}}
    ]}})).unwrap();
    project.put_part("blocks".into(), part).unwrap();
    assert!((project.bodies[0].volume - 3000.).abs() < 1e-5);
    assert!((project.bodies[0].bounds[1][0] - 50.).abs() < 1e-5);
    project
        .set_params([("copies".into(), "4".into()), ("pitch".into(), "5".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 2500.).abs() < 1e-5);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 2500.).abs() < 1e-5);
    let saved = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for count in ["0", "-1", "2.5", "1001"] {
        assert!(
            project
                .set_params([("copies".into(), count.into())].into())
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            saved
        );
    }
    assert!(
        project
            .set_params([("pitch".into(), "0".into())].into())
            .is_err()
    );
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        saved
    );
    project
        .set_params([("copies".into(), "1".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 1000.).abs() < 1e-5);
}
