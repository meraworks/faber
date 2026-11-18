use faber_model::Placement;
use faber_model::motion::{MotionStudy, Pose};
use glam::{DQuat, DVec3, EulerRot};
use serde_json::{Value, json};
fn joint(kind: &str) -> Value {
    json!({"kind":kind,"parts":["moving"],"axis":[0,0,1],"origin":[0,0,0]})
}
fn study(j: Value) -> MotionStudy {
    serde_json::from_value(json!({"joints":{"hinge":j}})).unwrap()
}
fn solve(
    s: &MotionStudy,
    value: Value,
) -> Result<std::collections::BTreeMap<String, Placement>, String> {
    s.solve_pose(&serde_json::from_value(json!({"hinge":value})).unwrap())
}
fn point(p: &Placement, xyz: [f64; 3]) -> DVec3 {
    let [x, y, z] = p.rotation.map(f64::to_radians);
    DQuat::from_euler(EulerRot::ZYX, z, y, x) * DVec3::from(xyz) + DVec3::from(p.translation)
}
fn check(p: &Placement, xyz: [f64; 3], want: [f64; 3]) {
    assert!(point(p, xyz).distance(want.into()) < 1e-9, "{p:?}");
}
#[test]
fn cylindrical_turns_and_slides() {
    let mut j = joint("cylindrical");
    j["origin"] = json!([2, 0, 0]);
    j["limits2"] = json!([-5, 5]);
    let s = study(j);
    let p = solve(&s, json!([90, 5])).unwrap();
    check(&p["moving"], [12., 0., 0.], [2., 10., 5.]);
    assert!(solve(&s, json!([90, 6])).is_err());
}
#[test]
fn pin_slot_slides_along_the_second_axis() {
    let mut j = joint("pin_slot");
    j["second_axis"] = json!([0, 2, 0]);
    let p = solve(&study(j), json!([90, 5])).unwrap();
    check(&p["moving"], [10., 0., 0.], [0., 15., 0.]);
}
#[test]
fn planar_moves_in_its_plane() {
    let mut j = joint("planar");
    j["second_axis"] = json!([0, 1, 0]);
    j["limits3"] = json!([-90, 90]);
    let s = study(j);
    let p = solve(&s, json!([3, 4, 90])).unwrap();
    check(&p["moving"], [10., 0., 0.], [-4., 13., 0.]);
    assert!(solve(&s, json!([3, 4, 91])).is_err());
}
#[test]
fn ball_turns_about_a_vector() {
    let s = study(joint("ball"));
    let p = solve(&s, json!([0, 0, 90])).unwrap();
    check(&p["moving"], [10., 0., 0.], [0., 10., 0.]);
    let p = solve(&s, json!([90., 90., 0.])).unwrap();
    let r = DQuat::from_scaled_axis(DVec3::new(90., 90., 0.) * std::f64::consts::PI / 180.);
    check(&p["moving"], [0., 0., 1.], (r * DVec3::Z).into());
}
#[test]
fn ball_limit_is_a_cone() {
    let mut j = joint("ball");
    j["limits"] = json!([0, 30]);
    let s = study(j);
    assert!(solve(&s, json!([0, 0, 90])).is_ok());
    let e = solve(&s, json!([45, 0, 0])).unwrap_err();
    assert!(
        e.contains("45") && e.contains("30") && e.contains("cone"),
        "{e}"
    );
}
#[test]
fn rigid_group_moves_as_one() {
    let mut s = study(joint("slider"));
    let mut j = joint("rigid");
    j["parts"] = json!(["child"]);
    j["parent"] = json!("hinge");
    s.joints
        .insert("rig".into(), serde_json::from_value(j).unwrap());
    let p = s
        .solve_pose(&serde_json::from_value(json!({"hinge":3,"rig":[]})).unwrap())
        .unwrap();
    assert_eq!(p["moving"], p["child"]);
    check(&p["child"], [1., 1., 0.], [1., 1., 3.]);
}
#[test]
fn old_single_number_pose_loads_and_saves_identical() {
    let serial = r#"{"hinge":12.5}"#;
    let pose: Pose = serde_json::from_str(serial).unwrap();
    assert_eq!(serde_json::to_string(&pose).unwrap(), serial);
    let scalar = solve(&study(joint("slider")), json!(12.5)).unwrap();
    check(&scalar["moving"], [0.; 3], [0., 0., 12.5]);
}
#[test]
fn wrong_value_count_is_refused() {
    let s = study(joint("cylindrical"));
    assert_eq!(
        solve(&s, json!(3)).unwrap_err(),
        "Joint hinge is cylindrical. A pose needs 2 values. It has 1."
    );
}
#[test]
fn invalid_joint_axes_limits_and_relations_are_refused() {
    for kind in ["pin_slot", "planar"] {
        let mut j = joint(kind);
        j["second_axis"] = json!([0, 0, 1]);
        assert!(study(j).solve_pose(&Pose::new()).is_err());
    }
    let mut j = joint("ball");
    j["limits"] = json!([0, 200]);
    assert!(study(j).solve_pose(&Pose::new()).is_err());
    let mut s = study(joint("slider"));
    let mut multi = joint("cylindrical");
    multi["parts"] = json!([]);
    s.joints
        .insert("other".into(), serde_json::from_value(multi).unwrap());
    s.gears.insert(
        "other".into(),
        serde_json::from_value(json!({"driver":"hinge","ratio":2})).unwrap(),
    );
    assert!(s.solve_pose(&Pose::new()).is_err());
}

#[test]
fn a_scalar_gear_can_drive_a_slide_and_a_slider_linkage_keeps_its_pin_distance() {
    let mut s = study(joint("slider"));
    let mut next = joint("slider");
    next["parts"] = json!(["rack"]);
    s.joints
        .insert("rack".into(), serde_json::from_value(next).unwrap());
    s.gears.insert(
        "rack".into(),
        serde_json::from_value(json!({"driver":"hinge","ratio":-2})).unwrap(),
    );
    check(
        &solve(&s, json!(3)).unwrap()["rack"],
        [0.; 3],
        [0., 0., -6.],
    );
    s.gears.clear();
    s.linkages.insert("bar".into(),serde_json::from_value(json!({"driver":"hinge","follower":"rack","driver_pin":[0,0,0],"follower_pin":[3,0,4],"rod_parts":["rod"]})).unwrap());
    let p = solve(&s, json!(2)).unwrap();
    check(&p["rod"], [0.; 3], [0., 0., 2.]);
    check(&p["rod"], [3., 0., 4.], [3., 0., 6.]);
    check(&p["rack"], [3., 0., 4.], [3., 0., 6.]);
}
