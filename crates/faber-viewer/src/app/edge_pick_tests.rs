use super::controls_fixture::{canvas, centre, click, two_blocks};
use super::*;
use egui_kittest::Harness;

fn left_rim(h: &mut Harness<'_, Shell>) -> egui::Pos2 {
    h.state_mut().set_filter(SelectFilter::Edge);
    let r = canvas(h);
    let edge = move_tool::screen_point(
        r,
        h.state().camera.matrix(r.aspect_ratio()),
        [-22.5, 0., 15.].into(),
    )
    .unwrap();
    edge + egui::vec2(0., 2.)
}

#[test]
fn edge_filter_picks_the_edge_near_the_cursor() {
    let (_d, mut h) = two_blocks();
    let p = left_rim(&mut h);
    h.hover_at(p);
    h.step();
    let pick = h.state().hover_pick.clone().unwrap();
    assert!(matches!(pick,Pick::Edge{ref part,edge} if part=="left" && edge>0));
    click(
        &mut h,
        p,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert_eq!(h.state().picks, vec![pick]);
}

#[test]
fn edge_pick_ignores_hidden_parts() {
    let (_d, mut h) = two_blocks();
    let p = left_rim(&mut h);
    assert!(h.state().pick_entity(canvas(&h), p).is_some());
    h.state_mut().hidden.insert("left".into());
    assert!(h.state().pick_entity(canvas(&h), p).is_none());
}

#[test]
fn a_hidden_edge_does_not_mask_a_visible_edge_in_the_same_part() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().set_filter(SelectFilter::Edge);
    let r = canvas(&h);
    let cursor = centre(&h, "left");
    let point = |dx: f32, dy: f32, z: f64| {
        let p = cursor + egui::vec2(dx, dy);
        let ndc = [
            2. * (p.x - r.left()) / r.width() - 1.,
            1. - 2. * (p.y - r.top()) / r.height(),
        ];
        let (eye, ray) = h.state().camera.ray(r.aspect_ratio(), ndc);
        (eye + ray * ((z - eye.z) / ray.z)).to_array()
    };
    let lines = faber_ports::EdgeLines {
        points: vec![
            point(-12., 0., 0.),
            point(12., 0., 0.),
            point(-12., 3., 15.),
            point(12., 3., 15.),
        ],
        ranges: vec![0..2, 2..4],
    };
    let bodies = std::sync::Arc::make_mut(&mut h.state_mut().bodies);
    bodies
        .iter_mut()
        .find(|body| body.id == "left")
        .unwrap()
        .edge_lines = lines;
    let picked = h.state().pick_entity(r, cursor).map(|(pick, _)| pick);
    assert_eq!(
        picked,
        Some(Pick::Edge {
            part: "left".into(),
            edge: 2
        })
    );
}
