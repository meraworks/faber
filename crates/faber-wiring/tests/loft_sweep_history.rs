use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn loft_and_sweep_faces_belong_to_the_solid_with_holes() {
    let profile = json!({"outer":{"kind":"circle","centre":[0,0],"radius":3},"holes":[{"kind":"circle","centre":[0,0],"radius":1}]});
    for operation in ["loft_sketches", "sweep_sketch"] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Curved face history".into())).unwrap();
        project
            .set_params([("height".into(), "10".into())].into())
            .unwrap();
        let (second, solid) = if operation == "loft_sketches" {
            (
                json!({"op":"sketch","profile":profile,"placement":{"translation":[0,0,"height"]}}),
                json!({"op":operation,"sources":["profile","second"],"ruled":true}),
            )
        } else {
            (
                json!({"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[0,0,"height"]}]}}),
                json!({"op":operation,"source":"profile","path":"second"}),
            )
        };
        let recipe = json!({"name":"Tube cap","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":profile}},
            {"id":"second","geometry":second},
            {"id":"body","geometry":solid},
            {"id":"cap","geometry":{"op":"thicken","source":"body","thickness":1,"face":{"created_by":"body","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}
        ]}});
        project
            .put_part(
                "tube".into(),
                serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
            )
            .unwrap();
        for height in [10, 15] {
            project
                .set_params([("height".into(), height.to_string())].into())
                .unwrap();
            assert!((project.bodies[0].volume - 8. * std::f64::consts::PI).abs() < 1e-5);
            assert!((project.bodies[0].bounds[0][2] - f64::from(height)).abs() < 1e-5);
            let (_, faces) = project.feature_topology("tube", "body").unwrap();
            assert_eq!(faces.len(), 4);
            assert!(
                faces
                    .iter()
                    .all(|face| face.created_by.as_deref() == Some(&["body".into()]))
            );
        }
        let before = std::fs::read(directory.path().join("faber.json")).unwrap();
        let mut invalid = recipe;
        invalid["geometry"]["features"][3]["geometry"]["face"]["created_by"] = json!("profile");
        assert!(
            project
                .put_part(
                    "tube".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.path().join("faber.json")).unwrap(),
            before
        );
        drop(project);
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        assert!((bodies[0].volume - 8. * std::f64::consts::PI).abs() < 1e-5);
    }
}
