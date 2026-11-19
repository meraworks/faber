#[allow(dead_code)]
mod face_edit_fixture;
use face_edit_fixture::*;
use serde_json::{Value, json};
use std::f64::consts::PI;
fn plane(x: f64) -> Value {
    feature(
        "mid",
        json!({"op":"plane","placement":{"translation":[x,0,0],"rotation":[0,90,0]}}),
    )
}
fn plane_split() -> Value {
    split(json!({"kind":"plane","plane":"mid"}))
}
fn split(tool: Value) -> Value {
    feature(
        "split",
        json!({"op":"split_face","body":"base","face":top(),"tool":tool}),
    )
}
fn disc() -> Vec<Value> {
    vec![
        blank(),
        feature(
            "circle",
            json!({"op":"sketch","profile":{"outer":{"kind":"circle","centre":[10,10],"radius":5}}}),
        ),
        split(json!({"kind":"sketch","sketch":"circle"})),
    ]
}
#[test]
fn plane_splits_the_top() {
    let (p, _dir) = build(&[blank(), plane(10.), plane_split()]);
    let faces = p.feature_topology("plate", "split").unwrap().1;
    assert_eq!(faces.len(), 7);
    assert_eq!(
        faces
            .iter()
            .filter(|f| (f.area - 200.).abs() < 1e-6)
            .count(),
        2
    );
    close(p.bodies[0].volume, 8000.);
}
#[test]
fn sketch_circle_splits_the_top() {
    let (p, _dir) = build(&disc());
    let faces = p.feature_topology("plate", "split").unwrap().1;
    assert_eq!(faces.len(), 7);
    assert!(faces.iter().any(|f| (f.area - 25. * PI).abs() < 1e-6));
    close(p.bodies[0].volume, 8000.);
}
#[test]
fn split_face_can_be_drafted() {
    let (p, _dir) = build(&[
        blank(),
        plane(10.),
        plane_split(),
        feature(
            "slope",
            json!({"op":"draft","source":"split","faces":{"near_centre":[15,10,20],"tolerance":0.001,"expect":1},"direction":[1,0,0],"origin":[10,0,20],"normal":[1,0,0],"angle":5}),
        ),
    ]);
    assert!((p.bodies[0].volume - 8000.).abs() > 1.);
    assert!(
        p.feature_topology("plate", "slope")
            .unwrap()
            .1
            .iter()
            .any(|f| f.normal.is_some_and(|n| n[2] > 0.99999) && (f.centre[0] - 5.).abs() < 1e-6)
    );
}
#[test]
fn split_face_can_be_moved() {
    let mut features = disc();
    features.push(feature("pad",json!({"op":"move_faces","body":"split","faces":[{"created_by":"split","area":25.*PI,"expect":1,"tolerance":0.001}],"distance":1})));
    let (p, _dir) = build(&features);
    close(p.bodies[0].volume, 8000. + 25. * PI);
}
#[test]
fn plane_that_misses_the_face_is_refused() {
    refused(
        &[blank(), plane(22.), plane_split()],
        "does not cross the selected face",
    )
}
#[test]
fn face_selectors_find_both_halves() {
    let (p, _dir) = build(&[blank(), plane(10.), plane_split()]);
    let (_, faces) = p.feature_topology("plate", "split").unwrap();
    let split: Vec<_> = faces
        .iter()
        .filter(|f| {
            f.created_by
                .as_ref()
                .is_some_and(|ids| ids.iter().any(|id| id == "split"))
        })
        .collect();
    assert_eq!(split.len(), 2);
    assert!(split.iter().all(|f| (f.area - 200.).abs() < 1e-6));
}
#[test]
fn two_split_regions_move_without_the_middle_strip() {
    let features = [
        blank(),
        plane(5.),
        plane_split(),
        feature(
            "far",
            json!({"op":"plane","placement":{"translation":[15,0,0],"rotation":[0,90,0]}}),
        ),
        feature(
            "third",
            json!({"op":"split_face","body":"split","face":{"near_centre":[12.5,10,20],"tolerance":0.001,"expect":1},"tool":{"kind":"plane","plane":"far"}}),
        ),
        feature(
            "lift",
            json!({"op":"move_faces","body":"third","faces":[{"normal":[0,0,1],"area":100,"tolerance":0.001,"expect":2}],"distance":1}),
        ),
    ];
    let (p, _dir) = build(&features);
    close(p.bodies[0].volume, 8200.);
    let (_, faces) = p.feature_topology("plate", "lift").unwrap();
    assert!(
        faces
            .iter()
            .any(|f| (f.area - 200.).abs() < 1e-6 && (f.centre[2] - 20.).abs() < 1e-6)
    );
}
