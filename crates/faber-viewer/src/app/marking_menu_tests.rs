use super::controls_fixture::{canvas, click, two_blocks};
use super::*;
use egui::{Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;

#[test]
fn right_click_shows_eight_entries_and_escape_closes_the_marking_menu() {
    let (_dir, mut h) = two_blocks();
    let pos = canvas(&h).center();
    click(&mut h, pos, PointerButton::Secondary, Modifiers::NONE);
    for action in [
        "Repeat last command",
        "Press pull",
        "Undo",
        "Redo",
        "Move/Copy",
        "Hole",
        "New sketch",
        "Delete",
    ] {
        assert!(
            h.query_all_by_label_contains(action).count() > 0,
            "Missing {action}"
        );
    }
    h.key_press(egui::Key::Escape);
    h.step();
    assert!(h.query_by_label("Repeat last command").is_none());
}

fn drag(h: &mut egui_kittest::Harness<'_, Shell>, start: egui::Pos2, delta: Vec2) {
    let end = start + delta;
    h.hover_at(start);
    h.step();
    for (pos, pressed) in [(start, true), (end, false)] {
        h.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
        if pressed {
            h.hover_at(end);
            h.step();
        }
    }
    h.step();
}

#[test]
fn gesture_up_runs_the_top_entry_and_keeps_the_camera_target() {
    let (_dir, mut h) = two_blocks();
    h.key_press(egui::Key::F);
    h.step();
    let fitted = h.state().camera;
    h.state_mut().camera.distance *= 1.5;
    let start = canvas(&h).center();
    drag(&mut h, start, Vec2::new(0., -68.));
    assert_eq!(h.state().camera, fitted);
    assert!(h.query_by_label("Repeat last command").is_none());
}

#[test]
fn repeat_runs_the_last_command() {
    let (_dir, mut h) = two_blocks();
    h.key_press(egui::Key::F);
    h.step();
    let expected = h.state().camera;
    h.state_mut().camera.distance *= 2.;
    let start = canvas(&h).center();
    click(&mut h, start, PointerButton::Secondary, Modifiers::NONE);
    h.get_by_label("Repeat last command").click();
    h.step();
    h.step();
    assert_eq!(h.state().camera, expected);
}

#[test]
fn move_context_row_opens_one_command_and_keeps_the_part() {
    let (_dir, mut h) = two_blocks();
    let point = super::controls_fixture::centre(&h, "left");
    click(&mut h, point, PointerButton::Secondary, Modifiers::NONE);
    h.get_by_label("Move/Copy M").click();
    h.step();
    h.step();
    assert!(matches!(h.state().marking.last, Some(keys::Command::Move)));
    assert!(h.state().pending.is_none());
    assert!(h.state().move_draft.is_some());
    assert!(h.state().data.parts.contains_key("left"));
}

#[test]
fn disabled_entry_does_nothing() {
    let (_dir, mut h) = two_blocks();
    let start = canvas(&h).center();
    let revision = h.state().data.revision;
    let camera = h.state().camera;
    drag(&mut h, start, Vec2::new(-65., -65.));
    assert_eq!(h.state().data.revision, revision);
    assert_eq!(h.state().camera, camera);
    assert!(h.state().move_draft.is_none());
}
