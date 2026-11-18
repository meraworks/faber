use super::controls_fixture::{canvas, two_blocks};
use super::face_pick::SelectFilter;
use super::*;
use egui_kittest::kittest::Queryable;
use faber_model::{Geometry, Scalar};

fn select_top(h: &mut egui_kittest::Harness<'_, Shell>) {
    let rect = canvas(h);
    let position = super::move_tool::screen_point(
        rect,
        h.state().camera.matrix(rect.aspect_ratio()),
        [-26., 6., 15.].into(),
    )
    .unwrap();
    h.state_mut()
        .pick_canvas(rect, position, false, false, false, false);
    let start = std::time::Instant::now();
    while !h.state().window.dirty && h.state().window.error.is_none() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "The pick did not reach the form."
        );
        h.step();
    }
    assert!(
        h.state().window.error.is_none(),
        "{:?}",
        h.state().window.error
    );
}

#[test]
fn hole_from_a_picked_point() {
    let (dir, mut h) = two_blocks();
    h.state_mut().new_feature("left", ToolKind::Hole).unwrap();
    h.step();
    h.get_by_role_and_label(egui::Role::Button, "Face");
    assert!(matches!(h.state().select_filter, SelectFilter::Face));
    select_top(&mut h);
    let Geometry::Hole { placement, .. } = h
        .state()
        .feature_draft
        .as_ref()
        .unwrap()
        .geometry()
        .unwrap()
    else {
        panic!("Expected a hole.")
    };
    for (actual, expected) in placement.translation.iter().zip([4., 6., 15.]) {
        let Scalar::Number(value) = actual else {
            panic!("Expected a coordinate.")
        };
        assert!((value - expected).abs() < 1e-4);
    }
    h.get_by_label("Apply feature").click();
    let start = std::time::Instant::now();
    while h.state().feature_draft.is_some() {
        assert!(
            h.state().window.error.is_none(),
            "{:?}",
            h.state().window.error
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        h.step();
    }
    let saved = faber_wiring::open(dir.path(), false, None).unwrap();
    let original = 15_f64.powi(3);
    let removed = std::f64::consts::PI * 2_f64.powi(2) * 10.;
    assert!(
        (saved.bodies.iter().find(|b| b.id == "left").unwrap().volume - (original - removed)).abs()
            < 1e-5
    );
    assert!(matches!(h.state().select_filter, SelectFilter::Body));
}

#[test]
fn sketch_on_a_picked_face() {
    let (_dir, mut h) = two_blocks();
    let draft = crate::sketch::SketchDraft::new(h.state().data.clone());
    h.state_mut().request_window(Draft::Sketch(draft));
    select_top(&mut h);
    let draft = h.state_mut().sketch_draft.as_mut().unwrap();
    draft.click([0., 0.]);
    draft.click([2., 3.]);
    let part = serde_json::to_value(draft.part().unwrap()).unwrap();
    assert_eq!(
        part["geometry"]["features"][0]["geometry"]["placement"]["translation"][2],
        15.
    );
}

#[test]
fn purchased_part_cannot_be_the_target() {
    let (_dir, mut h) = two_blocks();
    let purchased=serde_json::from_value(serde_json::json!({
        "op":"imported","asset":"asset",
        "purchased":{"source":"https://example.test","licence":"Test licence","designation":"Test part"}
    })).unwrap();
    h.state_mut().data.parts.get_mut("left").unwrap().geometry = purchased;
    assert_eq!(
        h.state_mut()
            .new_feature("left", ToolKind::Fillet)
            .unwrap_err(),
        "Part left is a purchased part. It cannot change."
    );
}
