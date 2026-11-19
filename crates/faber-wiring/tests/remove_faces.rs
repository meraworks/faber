#[allow(dead_code)]
mod face_edit_fixture;
use face_edit_fixture::*;
use serde_json::{Value, json};

fn remove(body: &str, faces: Vec<Value>) -> Value {
    feature(
        "heal",
        json!({"op":"remove_faces","body":body,"faces":faces}),
    )
}
fn verify(features: &[Value]) {
    let (p, _dir) = build(features);
    close(p.bodies[0].volume, 8000.);
    assert_eq!(p.feature_topology("plate", "heal").unwrap().1.len(), 6);
}
#[test]
fn hole_is_removed() {
    verify(&[blank(), drilled(), remove("hole", vec![cylinder(2.5)])])
}
#[test]
fn fillet_is_removed() {
    verify(&[
        blank(),
        rounded("base"),
        remove("round", vec![cylinder(2.)]),
    ])
}
#[test]
fn boss_is_removed() {
    verify(&[
        blank(),
        feature("peg", json!({"op":"cylinder","radius":3,"height":5})),
        feature(
            "placed",
            json!({"op":"transform","source":"peg","placement":{"translation":[10,10,20],"rotation":[0,0,0]}}),
        ),
        feature(
            "joined",
            json!({"op":"union","left":"base","right":"placed"}),
        ),
        remove(
            "joined",
            vec![
                cylinder(3.),
                json!({"near_centre":[10,10,25],"tolerance":0.001,"expect":1}),
            ],
        ),
    ])
}
#[test]
fn removal_that_cannot_heal_is_refused() {
    refused(
        &[blank(), remove("base", vec![top()])],
        "The faces cannot be removed: the neighbour faces do not close the solid.",
    )
}
#[test]
fn later_selector_still_works() {
    let (p, _dir) = build(&[
        blank(),
        drilled(),
        remove("hole", vec![cylinder(2.5)]),
        rounded("heal"),
    ]);
    close(
        p.bodies[0].volume,
        8000. - 20. * (4. - std::f64::consts::PI),
    );
}
