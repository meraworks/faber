use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn extrude_and_revolve_faces_belong_to_the_solid_feature() {
    for (profile, operation, area, count) in [
        (
            json!({"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[20,20]}}}),
            json!({"op":"extrude_sketch","source":"profile","vector":[0,0,"height"]}),
            400.,
            6,
        ),
        (
            json!({"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"rectangle","origin":[2,0],"size":[2,"height"]}}}),
            json!({"op":"revolve_sketch","source":"profile","origin":[0,0,0],"axis":[0,0,1],"angle":360}),
            12. * std::f64::consts::PI,
            4,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Generated faces".into())).unwrap();
        project
            .set_params([("height".into(), "10".into())].into())
            .unwrap();
        let recipe = json!({"name":"Cap","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
            {"id":"profile","geometry":profile},
            {"id":"body","geometry":operation},
            {"id":"cap","geometry":{"op":"thicken","source":"body","thickness":1,"face":{"created_by":"body","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}
        ]}});
        let mut inspected = recipe.clone();
        inspected["geometry"]["features"]
            .as_array_mut()
            .unwrap()
            .pop();
        inspected["geometry"]["output"] = json!("body");
        project
            .put_part(
                "cap".into(),
                serde_json::from_value::<Part<Scalar>>(inspected).unwrap(),
            )
            .unwrap();
        let (_, caps) = project.feature_topology("cap", "body").unwrap();
        let bottom = caps
            .iter()
            .find(|face| face.centre[2].abs() < 1e-6)
            .unwrap();
        assert!(
            bottom.normal.is_some_and(|normal| normal[2] < -0.999),
            "Bottom face: {bottom:?}"
        );
        project
            .put_part(
                "cap".into(),
                serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
            )
            .unwrap_or_else(|error| panic!("{error}; {}", recipe["geometry"]["features"][1]));
        assert!((project.bodies[0].volume - area).abs() < 1e-5);
        let (_, faces) = project.feature_topology("cap", "body").unwrap();
        assert_eq!(faces.len(), count);
        assert!(
            faces
                .iter()
                .all(|face| face.created_by.as_deref() == Some(&["body".into()]))
        );
        project
            .set_params([("height".into(), "15".into())].into())
            .unwrap();
        assert!((project.bodies[0].bounds[0][2] - 15.).abs() < 1e-5);
        let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
        let mut invalid = recipe;
        invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("profile");
        assert!(
            project
                .put_part(
                    "cap".into(),
                    serde_json::from_value::<Part<Scalar>>(invalid).unwrap()
                )
                .is_err()
        );
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
        drop(project);
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        assert!((bodies[0].volume - area).abs() < 1e-5);
        assert!((bodies[0].bounds[0][2] - 15.).abs() < 1e-5);
    }
}
