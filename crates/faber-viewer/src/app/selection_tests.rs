use super::controls_fixture::*;
use super::*;
use egui::{Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;

#[test]
fn shift_click_on_the_canvas_adds_to_the_selection() {
    let (_dir, mut h) = two_blocks();
    let left = centre(&h, "left");
    let right = centre(&h, "right");
    click(&mut h, left, PointerButton::Primary, Modifiers::NONE);
    click(&mut h, right, PointerButton::Primary, Modifiers::SHIFT);
    assert_eq!(h.state().selection(), ["left", "right"]);
    click(&mut h, right, PointerButton::Primary, Modifiers::SHIFT);
    assert_eq!(h.state().selection(), ["left", "right"]);
}

#[test]
fn empty_space_menu_has_fit_and_new_sketch() {
    let (_dir, mut h) = two_blocks();
    let pos = canvas(&h).center();
    click(&mut h, pos, PointerButton::Secondary, Modifiers::NONE);
    h.get_by_label("Fit view");
    assert!(
        h.query_all_by_label("New sketch").count() >= 2,
        "The canvas menu must offer a sketch"
    );
}

#[test]
fn part_menu_has_edit_features() {
    let (_dir, mut h) = two_blocks();
    let pos = centre(&h, "right");
    click(&mut h, pos, PointerButton::Secondary, Modifiers::NONE);
    h.get_by_label("Edit features").click();
    h.step();
    assert_eq!(h.state().selected.as_deref(), Some("right"));
}

#[test]
fn hover_marks_the_body_under_the_cursor() {
    let (_dir, mut h) = two_blocks();
    let pos = centre(&h, "right");
    let colour = |h: &egui_kittest::Harness<'_, Shell>| {
        let target = h.state().target.as_ref().unwrap();
        let canvas = canvas(h);
        let x = ((pos.x - canvas.min.x) * target.width as f32 / canvas.width()) as usize;
        let y = ((pos.y - canvas.min.y) * target.height as f32 / canvas.height()) as usize;
        let offset = (y * target.width as usize + x) * 4;
        h.state().gpu.read(target).unwrap()[offset..offset + 3].to_vec()
    };
    let original = colour(&h);
    h.hover_at(pos);
    h.step();
    assert_ne!(colour(&h), original, "The hovered body must change colour");
    h.hover_at(canvas(&h).center());
    h.step();
    assert_eq!(colour(&h), original);
}
#[test]
fn menu_targets_the_clicked_member_of_a_group() {
    let (_dir, mut h) = two_blocks();
    let left = centre(&h, "left");
    let right = centre(&h, "right");
    click(&mut h, left, PointerButton::Primary, Modifiers::NONE);
    click(&mut h, right, PointerButton::Primary, Modifiers::SHIFT);
    click(&mut h, left, PointerButton::Secondary, Modifiers::NONE);
    h.get_by_label("Edit features");
    assert_eq!(h.state().selected.as_deref(), Some("left"));
    assert_eq!(h.state().selection(), ["left", "right"]);
}

#[test]
fn clearing_the_selection_updates_the_cached_view() {
    let (_dir, mut h) = two_blocks();
    let pos = centre(&h, "left");
    click(&mut h, pos, PointerButton::Primary, Modifiers::NONE);
    h.hover_at(canvas(&h).center());
    h.step();
    let frame = |h: &egui_kittest::Harness<'_, Shell>| {
        h.state()
            .gpu
            .read(h.state().target.as_ref().unwrap())
            .unwrap()
    };
    let selected = frame(&h);
    h.key_press(egui::Key::Escape);
    h.step();
    assert!(h.state().selected.is_none());
    assert!(
        frame(&h) != selected,
        "The last selection is still visible."
    );
}
