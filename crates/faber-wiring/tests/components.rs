use faber_core::{Motion, Project, component_parts, world_placement};
use faber_model::{Edit, Part, Scalar};
use glam::DVec3;
use serde_json::{Value, json};

fn component(id: &str, parent: Option<&str>, at: [f64; 3], rz: f64) -> Edit {
    serde_json::from_value(json!({"tool":"set_component","id":id,"component":{
        "name":id,"parent":parent,"placement":{"translation":at,"rotation":[0,0,rz]}
    }}))
    .unwrap()
}
fn part(id: &str, parent: &str, x: f64) -> Edit {
    let part: Part<Scalar> = serde_json::from_value(json!({"name":id,"component":parent,
        "colour":[20,40,50],"placement":{"translation":[x,0,0]},
        "geometry":{"op":"box","size":[2,4,6]}}))
    .unwrap();
    Edit::PutPart {
        id: id.into(),
        part,
    }
}
fn with_parts() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Assembly".into())).unwrap();
    p.batch(vec![
        component("a", None, [10., 0., 0.], 90.),
        component("b", Some("a"), [0.; 3], 0.),
        part("first", "a", 5.),
        part("second", "b", 12.),
    ])
    .unwrap();
    (dir, p)
}
#[test]
fn nested_placement() {
    let (_dir, p) = with_parts();
    let x = world_placement(&p.data, "first").unwrap();
    assert!((x.translation - DVec3::new(10., 5., 0.)).length() < 1e-9);
    let b = &p.bodies.iter().find(|b| b.id == "first").unwrap().bounds;
    for (p, q) in b.iter().zip([[6., 5., 0.], [10., 7., 6.]]) {
        assert!((DVec3::from(*p) - DVec3::from(q)).length() < 1e-6);
    }
    assert_eq!(component_parts(&p.data, "a", false), ["first"]);
    assert_eq!(component_parts(&p.data, "a", true), ["first", "second"]);
}
#[test]
fn component_move_moves_the_parts_in_one_revision() {
    let (_dir, mut p) = with_parts();
    let revision = p.data.revision;
    p.move_component("a", &Motion::Translate([0., 2., 3.]))
        .unwrap();
    assert_eq!(p.data.revision, revision + 1);
    for (id, y) in [("first", 7.), ("second", 14.)] {
        assert!(
            (world_placement(&p.data, id).unwrap().translation - DVec3::new(10., y, 3.)).length()
                < 1e-9
        );
    }
    p.move_part("second", Motion::Translate([2., 0., 0.]), None)
        .unwrap();
    assert!(
        (world_placement(&p.data, "second").unwrap().translation - DVec3::new(12., 14., 3.))
            .length()
            < 1e-9
    );
}
#[test]
fn component_loop_is_refused() {
    let (_dir, mut p) = with_parts();
    let err = p
        .batch(vec![component("a", Some("b"), [10., 0., 0.], 90.)])
        .unwrap_err();
    assert_eq!(err, "Component a is its own parent through: a, b.");
}
#[test]
fn remove_component_with_parts() {
    let (_dir, mut p) = with_parts();
    let remove = |keep| {
        serde_json::from_value(json!({"tool":"remove_component","id":"a","keep_parts":keep}))
            .unwrap()
    };
    assert!(p.batch(vec![remove(false)]).unwrap_err().contains("first"));
    let before = p.bodies.iter().map(|b| b.bounds).collect::<Vec<_>>();
    p.batch(vec![remove(true)]).unwrap();
    assert_eq!(p.data.components["b"].parent, None);
    assert_eq!(p.data.parts["first"].component, None);
    assert_eq!(p.data.parts["second"].component.as_deref(), Some("b"));
    for (b, original) in p.bodies.iter().zip(before) {
        for (actual, expected) in b.bounds.into_iter().zip(original) {
            assert!((DVec3::from(actual) - DVec3::from(expected)).length() < 1e-6);
        }
    }
}
#[test]
fn project_without_components_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Legacy".into())).unwrap();
    let mut data = serde_json::to_value(&p.data).unwrap();
    assert!(data.get("components").is_none());
    let mut base: Value =
        json!({"name":"Part","colour":[20,40,50],"geometry":{"op":"box","size":[2,4,6]}});
    p.put_part(
        "p".into(),
        serde_json::from_value::<Part<Scalar>>(base.take()).unwrap(),
    )
    .unwrap();
    let before = std::fs::read(dir.path().join("parts/p.json")).unwrap();
    let old_manifest = std::fs::read(dir.path().join("faber.json")).unwrap();
    drop(p);
    let p = faber_wiring::open(dir.path(), false, None).unwrap();
    data = serde_json::to_value(&p.data).unwrap();
    assert!(data["parts"]["p"].get("component").is_none());
    assert_eq!(
        before,
        std::fs::read(dir.path().join("parts/p.json")).unwrap()
    );
    assert_eq!(
        old_manifest,
        std::fs::read(dir.path().join("faber.json")).unwrap()
    );
}

#[test]
fn reparent_keeps_the_world_pose_and_component_alignment_moves_the_subtree() {
    let (_dir, mut p) = with_parts();
    let original = world_placement(&p.data, "second").unwrap();
    p.set_part_info(faber_core::PartInfo {
        id: "second".into(),
        component: Some(String::new()),
        ..Default::default()
    })
    .unwrap();
    assert!(
        world_placement(&p.data, "second")
            .unwrap()
            .abs_diff_eq(original, 1e-9)
    );
    p.set_part_info(faber_core::PartInfo {
        id: "second".into(),
        component: Some("b".into()),
        ..Default::default()
    })
    .unwrap();
    assert!(
        world_placement(&p.data, "second")
            .unwrap()
            .abs_diff_eq(original, 1e-9)
    );
    p.align_component(
        "b",
        "first",
        &[faber_core::Axis::Y],
        faber_core::AlignMode::MinToMax,
        3.,
    )
    .unwrap();
    let fixed = p.bodies.iter().find(|b| b.id == "first").unwrap();
    let aligned = p.bodies.iter().find(|b| b.id == "second").unwrap();
    assert!((aligned.bounds[0][1] - fixed.bounds[1][1] - 3.).abs() < 1e-6);
}

#[test]
fn removing_or_reparenting_a_component_keeps_the_position_parameter() {
    for remove in [false, true] {
        let (_dir, mut p) = with_parts();
        let mut first = p.data.parts["first"].clone();
        first.placement.translation[0] = Scalar::Expression("L + 1".into());
        p.batch(vec![
            Edit::SetParam {
                name: "L".into(),
                expression: "3".into(),
            },
            Edit::PutPart {
                id: "first".into(),
                part: first,
            },
        ])
        .unwrap();
        if remove {
            p.batch(vec![Edit::RemoveComponent {
                id: "a".into(),
                keep_parts: true,
            }])
            .unwrap();
        } else {
            p.set_part_info(faber_core::PartInfo {
                id: "first".into(),
                component: Some(String::new()),
                ..Default::default()
            })
            .unwrap();
        }
        p.batch(vec![Edit::SetParam {
            name: "L".into(),
            expression: "8".into(),
        }])
        .unwrap();
        let at = world_placement(&p.data, "first").unwrap().translation;
        assert!(at.abs_diff_eq(DVec3::new(10., 9., 0.), 1e-9), "{at}");
    }
}

#[test]
fn changing_a_parent_never_discards_an_angle_expression() {
    let (_dir, mut p) = with_parts();
    let mut part = p.data.parts["first"].clone();
    part.placement.rotation[1] = Scalar::Expression("tilt".into());
    p.batch(vec![
        Edit::SetParam {
            name: "tilt".into(),
            expression: "30".into(),
        },
        Edit::PutPart {
            id: "first".into(),
            part,
        },
    ])
    .unwrap();
    p.set_part_info(faber_core::PartInfo {
        id: "first".into(),
        component: Some("b".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        p.data.parts["first"].placement.rotation[1],
        Scalar::Expression("tilt".into())
    );
    let before = p.data.clone();
    let err = p
        .set_part_info(faber_core::PartInfo {
            id: "first".into(),
            component: Some(String::new()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.contains("rotation[1]"), "{err}");
    assert_eq!(before, p.data);
}
