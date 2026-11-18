use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn dress_operations_preserve_old_faces_and_name_new_faces() {
    for operation in ["fillet", "chamfer"] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Dress history".into())).unwrap();
        project
            .set_params([("width".into(), "10".into())].into())
            .unwrap();
        let mut dress = json!({"op":operation,"source":"base","edges":{"between":[
            {"normal":[0,0,1],"expect":1,"tolerance":0.000001},
            {"normal":[1,0,0],"expect":1,"tolerance":0.000001}
        ],"expect":1,"tolerance":0.000001}});
        dress[if operation == "fillet" {
            "radius"
        } else {
            "distance"
        }] = json!(2);
        let recipe = json!({"name":"Dressed box","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
            {"id":"base","geometry":{"op":"box","size":["width",20,30]}},
            {"id":"dress","geometry":dress},
            {"id":"cap","geometry":{"op":"thicken","source":"dress","thickness":1,"face":{"created_by":"base","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}
        ]}});
        project
            .put_part(
                "box".into(),
                serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
            )
            .unwrap();
        for (width, volume) in [(10, 160.), (15, 260.)] {
            project
                .set_params([("width".into(), width.to_string())].into())
                .unwrap();
            assert!((project.bodies[0].volume - volume).abs() < 1e-5);
            let (_, faces) = project.feature_topology("box", "dress").unwrap();
            assert_eq!(faces.len(), 7);
            assert_eq!(
                faces
                    .iter()
                    .filter(|face| face.created_by.as_deref() == Some(&["base".into()]))
                    .count(),
                6
            );
            let new = faces
                .iter()
                .filter(|face| face.created_by.as_deref() == Some(&["dress".into()]))
                .collect::<Vec<_>>();
            assert_eq!(new.len(), 1);
            let area = if operation == "fillet" {
                20. * std::f64::consts::PI
            } else {
                40. * 2_f64.sqrt()
            };
            assert!((new[0].area - area).abs() < 1e-5);
        }
        let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
        let mut invalid = recipe;
        invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("dress");
        assert!(
            project
                .put_part(
                    "box".into(),
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
        assert!((bodies[0].volume - 260.).abs() < 1e-5);
    }
}
