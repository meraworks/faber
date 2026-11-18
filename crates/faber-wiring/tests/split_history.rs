use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn split_tracks_old_faces_and_the_cut_face_after_plane_changes() {
    for keep in ["positive", "negative", "both"] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Split history".into())).unwrap();
        project
            .set_params([("height".into(), "12".into())].into())
            .unwrap();
        let recipe = json!({"name":"Split cap","colour":[100,140,170],"geometry":{"op":"history","output":"cap","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
            {"id":"split","geometry":{"op":"split","source":"base","origin":[0,0,"height"],"normal":[0,0,1],"keep":keep}},
            {"id":"cap","geometry":{"op":"thicken","source":"split","thickness":1,"face":{"created_by":"split","expect":1,"tolerance":0.000001}}}
        ]}});
        project
            .put_part(
                "box".into(),
                serde_json::from_value::<Part<Scalar>>(recipe.clone()).unwrap(),
            )
            .unwrap();
        for height in [12, 15] {
            project
                .set_params([("height".into(), height.to_string())].into())
                .unwrap();
            assert!((project.bodies[0].volume - 200.).abs() < 1e-5);
            let (_, faces) = project.feature_topology("box", "split").unwrap();
            let old_count = if keep == "both" { 10 } else { 5 };
            assert_eq!(faces.len(), old_count + 1);
            assert_eq!(
                faces
                    .iter()
                    .filter(|face| face.created_by.as_deref() == Some(&["base".into()]))
                    .count(),
                old_count
            );
            let cut = faces
                .iter()
                .filter(|face| face.created_by.as_deref() == Some(&["split".into()]))
                .collect::<Vec<_>>();
            assert_eq!(cut.len(), 1);
            assert!((cut[0].centre[2] - f64::from(height)).abs() < 1e-5);
            assert!((cut[0].area - 200.).abs() < 1e-5);
        }
        let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
        let mut invalid = recipe;
        invalid["geometry"]["features"][2]["geometry"]["face"]["created_by"] = json!("base");
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
        assert!((bodies[0].volume - 200.).abs() < 1e-5);
    }
}
