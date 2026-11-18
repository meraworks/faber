use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn thicken_preserves_selected_source_and_names_new_faces_in_both_directions() {
    for direction in [-1, 1] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Thicken history".into())).unwrap();
        project
            .set_params([("wall".into(), "2".into())].into())
            .unwrap();
        let recipe = json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
            {"id":"base","geometry":{"op":"box","size":[20,20,30]}},
            {"id":"plate","geometry":{"op":"thicken","source":"base","thickness":format!("{direction} * wall"),"face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001}}},
            {"id":"cap","geometry":{"op":"thicken","source":"plate","thickness":1,"face":{"created_by":"plate","normal":[0,0,direction],"expect":1,"tolerance":0.000001}}}
        ]}});
        project
            .put_part(
                "plate".into(),
                serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
            )
            .unwrap();
        for wall in [2, 3] {
            project
                .set_params([("wall".into(), wall.to_string())].into())
                .unwrap();
            assert!((project.bodies[0].volume - 400.).abs() < 1e-5);
            let z = 30. + f64::from(direction * wall) + if direction < 0 { -1. } else { 0. };
            assert!((project.bodies[0].bounds[0][2] - z).abs() < 1e-5);
            let (_, faces) = project.feature_topology("plate", "plate").unwrap();
            assert_eq!(faces.len(), 6);
            assert_eq!(
                faces
                    .iter()
                    .filter(|face| face.created_by.as_deref() == Some(&["base".into()]))
                    .count(),
                1
            );
            assert_eq!(
                faces
                    .iter()
                    .filter(|face| face.created_by.as_deref() == Some(&["plate".into()]))
                    .count(),
                5
            );
        }
        let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
        let mut invalid = recipe;
        invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("base");
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
        drop(project);
        let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
        assert!((bodies[0].volume - 400.).abs() < 1e-5);
    }
}
