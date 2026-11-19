#[allow(dead_code)]
mod face_edit_fixture;
use face_edit_fixture::*;
use serde_json::{Value, json};
fn offset(body: &str, faces: Vec<Value>, d: f64) -> Value {
    feature(
        "change",
        json!({"op":"move_faces","body":body,"faces":faces,"distance":d}),
    )
}
#[test]
fn top_face_up_by_two() {
    let (p, _dir) = build(&[blank(), offset("base", vec![top()], 2.)]);
    close(p.bodies[0].volume, 8800.);
    close(p.bodies[0].bounds[1][2], 22.);
}
#[test]
fn top_face_down_by_five() {
    let (p, _dir) = build(&[blank(), offset("base", vec![top()], -5.)]);
    close(p.bodies[0].volume, 6000.)
}
#[test]
fn hole_gets_larger() {
    let (p, _dir) = build(&[
        blank(),
        drilled(),
        offset("hole", vec![cylinder(2.5)], -0.5),
    ]);
    close(p.bodies[0].volume, 8000. - std::f64::consts::PI * 9. * 20.);
}
#[test]
fn two_opposite_faces() {
    let sides = [-1, 1].map(|x| json!({"normal":[x,0,0],"tolerance":0.001,"expect":1}));
    let (p, _dir) = build(&[blank(), offset("base", sides.to_vec(), 1.)]);
    close(p.bodies[0].bounds[1][0] - p.bodies[0].bounds[0][0], 22.);
    close(p.bodies[0].volume, 8800.);
}
#[test]
fn face_next_to_a_fillet() {
    let (p, _dir) = build(&[blank(), rounded("base"), offset("round", vec![top()], 2.)]);
    close(p.bodies[0].bounds[1][2], 22.);
    close(p.bodies[0].bounds[0][0], 0.);
    close(
        p.bodies[0].volume,
        8800. - 20. * (4. - std::f64::consts::PI),
    );
    assert!(
        p.feature_topology("plate", "change")
            .unwrap()
            .1
            .iter()
            .any(|f| f
                .cylinder
                .as_ref()
                .is_some_and(|c| (c.radius - 2.).abs() < 1e-6))
    );
}
#[test]
fn move_through_the_opposite_face_is_refused() {
    refused(
        &[blank(), offset("base", vec![top()], -25.)],
        "Distance -25 mm removes the body: the thickness at the face is 20 mm.",
    )
}
#[test]
fn thickness_is_measured_under_the_selected_region() {
    let features = [
        blank(),
        feature("channel", json!({"op":"box","size":[10,20,10]})),
        feature(
            "under",
            json!({"op":"transform","source":"channel","placement":{"translation":[10,0,0]}}),
        ),
        feature(
            "step",
            json!({"op":"difference","left":"base","right":"under"}),
        ),
        feature(
            "mid",
            json!({"op":"plane","placement":{"translation":[10,0,0],"rotation":[0,90,0]}}),
        ),
        feature(
            "region",
            json!({"op":"split_face","body":"step","face":top(),"tool":{"kind":"plane","plane":"mid"}}),
        ),
        offset(
            "region",
            vec![json!({"near_centre":[15,10,20],"expect":1,"tolerance":0.001})],
            -12.,
        ),
    ];
    refused(
        &features,
        "Distance -12 mm removes the body: the thickness at the face is 10 mm.",
    );
}
