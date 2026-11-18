use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn mirror_preserves_volume_across_an_oblique_offset_plane() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Mirror".into())).unwrap();
    project
        .set_params([("offset".into(), "10".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"history","output":"mirror","features":[
        {"id":"seed","geometry":{"op":"box","size":[2,3,4]}},
        {"id":"mirror","geometry":{"op":"mirror","source":"seed","origin":["offset",0,0],"normal":[1,1,0],"keep_source":false}}
    ]}})).unwrap();
    project.put_part("block".into(), part).unwrap();
    assert!((project.bodies[0].volume - 24.).abs() < 1e-5);
    for (actual, expected) in project.bodies[0]
        .bounds
        .iter()
        .flatten()
        .zip([7., 8., 0., 10., 10., 4.])
    {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    project
        .set_params([("offset".into(), "20".into())].into())
        .unwrap();
    assert!((project.bodies[0].bounds[0][0] - 17.).abs() < 1e-5);
    let both = serde_json::from_value(json!({"op":"mirror","source":"seed","origin":["offset",0,0],"normal":[1,1,0],"keep_source":true})).unwrap();
    project.edit_feature("block", "mirror", both).unwrap();
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 48.).abs() < 1e-5);
    let saved = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let invalid = serde_json::from_value(
        json!({"op":"mirror","source":"seed","origin":[0,0,0],"normal":[0,0,0],"keep_source":true}),
    )
    .unwrap();
    assert!(project.edit_feature("block", "mirror", invalid).is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        saved
    );
}
