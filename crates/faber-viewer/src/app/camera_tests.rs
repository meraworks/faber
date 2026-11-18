use super::controls_fixture::*;
use super::*;
use egui_kittest::kittest::Queryable;

#[test]
fn cube_face_click_gives_the_face_view() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.step();
    h.get_by_label("View cube Top").click();
    h.step();
    let c = h.state().camera;
    assert!(((c.target - c.eye()).normalize() + glam::Vec3::Z).length() < 1e-5);
}

#[test]
fn cube_corner_click_gives_a_corner_view() {
    let (_dir, mut h) = two_blocks();
    let pos = h.get_by_label("View cube Top front right").rect().center();
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    let c = h.state().camera;
    assert!(
        ((c.eye() - c.target).normalize() - glam::Vec3::new(1., -1., 1.).normalize()).length()
            < 1e-5
    );
}

#[test]
fn double_middle_click_fits() {
    let (_dir, mut h) = two_blocks();
    let distance = h.state().camera.distance;
    h.state_mut().camera.distance = distance * 0.25;
    let pos = canvas(&h).center();
    for _ in 0..2 {
        click(
            &mut h,
            pos,
            egui::PointerButton::Middle,
            egui::Modifiers::NONE,
        );
    }
    assert!((h.state().camera.distance - distance).abs() < 1e-4);
}
