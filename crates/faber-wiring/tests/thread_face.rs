use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn sleeve_faces_select_by_side_and_radius() {
    let root = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(root.path(), true, Some("Sleeve".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Sleeve","colour":[90,110,130],"geometry":{"op":"history","features":[
      {"id":"outer","geometry":{"op":"cylinder","radius":6,"height":12}},
      {"id":"bore","geometry":{"op":"cylinder","radius":3,"height":12}},
      {"id":"sleeve","geometry":{"op":"difference","left":"outer","right":"bore"}}],"output":"sleeve"}})).unwrap();
    project.put_part("body".into(), part).unwrap();
    for (internal, radius, count) in [(true, 3., 1), (false, 6., 1), (false, 3., 0)] {
        let selector = serde_json::from_value(
            json!({"internal":internal,"cylinder_radius":radius,"tolerance":0.00001,"expect":1}),
        )
        .unwrap();
        let (_, faces) = project
            .selected_topology("body", None, None, Some(&selector))
            .unwrap();
        assert_eq!(faces.len(), count);
    }
}

#[test]
fn selected_shaft_thread_removes_material_and_reopens() {
    let root = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(root.path(), true, Some("Thread".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Spindle","colour":[120,120,120],"geometry":{"op":"history","features":[
        {"id":"shaft","geometry":{"op":"cylinder","radius":3,"height":6}},
        {"id":"cut","geometry":{"op":"thread","source":"shaft","size":"M6","length":3,"offset":1,
          "face":{"cylinder_radius":3,"internal":false,"tolerance":0.00001,"expect":1}}}],"output":"cut"}})).unwrap();
    project.put_part("spindle".into(), part).unwrap();
    let volume = project.bodies[0].volume;
    assert!(volume < 54. * std::f64::consts::PI - 5. && volume > 120.);
    drop(project);
    let (_, bodies) = faber_wiring::read(root.path()).unwrap();
    assert!((volume - bodies[0].volume).abs() < 1e-6);
}

#[test]
fn bore_accepts_both_thread_directions() {
    let root = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(root.path(), true, Some("Threaded sleeve".into())).unwrap();
    for left in [false, true] {
        let part: Part<Scalar> = serde_json::from_value(json!({"name":"Sleeve","colour":[120,120,120],"geometry":{"op":"history","features":[
            {"id":"outer","geometry":{"op":"cylinder","radius":5,"height":6}},
            {"id":"bore","geometry":{"op":"cylinder","radius":2.5,"height":6}},
            {"id":"sleeve","geometry":{"op":"difference","left":"outer","right":"bore"}},
            {"id":"cut","geometry":{"op":"thread","source":"sleeve","size":"M6","length":4,"offset":1,"left_handed":left,"face":{"cylinder_radius":2.5,"internal":true,"tolerance":0.00001,"expect":1}}}],"output":"cut"}})).unwrap();
        project.put_part("sleeve".into(), part).unwrap();
        assert!(project.bodies[0].volume < (25. - 6.25) * 6. * std::f64::consts::PI - 5.);
    }
}
