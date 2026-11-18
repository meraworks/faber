use super::controls_fixture::{canvas, centre, click, two_blocks};
use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    for item in std::fs::read_dir(dir).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.insert(path.clone(), std::fs::read(path).unwrap());
        }
    }
    found
}

fn wait(h: &mut Harness<'_, Shell>, done: impl Fn(&Shell) -> bool) {
    let end = Instant::now() + Duration::from_secs(12);
    while !done(h.state()) {
        assert!(
            Instant::now() < end,
            "Press pull timed out: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
}

fn button(h: &mut Harness<'_, Shell>, pos: egui::Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
}

fn open_on_top_face(h: &mut Harness<'_, Shell>) {
    h.get_by_label("Face").click();
    h.step();
    let p = centre(h, "left");
    click(h, p, egui::PointerButton::Primary, egui::Modifiers::NONE);
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.state_mut().dirty = true;
    h.step();
    h.key_press(egui::Key::Q);
    h.step();
    assert!(
        h.state().press.is_some(),
        "Q opens the tool on the selected face"
    );
    wait(h, |s| s.press.as_ref().is_some_and(|t| t.face.is_some()));
}

fn drag_arrow(h: &mut Harness<'_, Shell>, mm: f64) -> egui::Pos2 {
    let (base, tip, scale) = h.state().press_arrow(canvas(h)).unwrap();
    let start = base + (tip - base) * 0.6;
    let travel = (tip - base).normalized() * (mm / scale) as f32;
    h.hover_at(start);
    h.step();
    button(h, start, true);
    for i in 1..=3 {
        h.hover_at(start + travel * (i as f32 / 3.));
        h.step();
    }
    let distance = h.state().press.as_ref().unwrap().distance;
    assert!((distance - mm).abs() < 0.001, "Arrow distance {distance}");
    start + travel
}

fn volume(bodies: &[Body], id: &str) -> f64 {
    bodies.iter().find(|b| b.id == id).unwrap().volume
}

#[test]
fn escape_during_a_drag_saves_nothing() {
    let (dir, mut h) = two_blocks();
    let original = files(dir.path());
    let revision = h.state().data.revision;
    open_on_top_face(&mut h);
    let pos = drag_arrow(&mut h, 3.);
    wait(&mut h, |s| {
        s.press
            .as_ref()
            .and_then(|t| t.preview.as_ref())
            .is_some_and(|b| (volume(b, "left") - 15. * 15. * 18.).abs() < 0.1)
    });
    assert_eq!(files(dir.path()), original);
    h.key_press(egui::Key::Escape);
    h.step();
    button(&mut h, pos, false);
    assert!(h.state().press.is_none());
    assert!(h.state().pending.is_none());
    assert_eq!(h.state().data.revision, revision);
    assert!((volume(&h.state().bodies, "left") - 15_f64.powi(3)).abs() < 1e-6);
    assert_eq!(files(dir.path()), original);
}

#[test]
fn drag_is_one_revision() {
    let (dir, mut h) = two_blocks();
    let revision = h.state().data.revision;
    open_on_top_face(&mut h);
    let pos = drag_arrow(&mut h, 4.);
    let distance = h.state().press.as_ref().unwrap().distance;
    assert_eq!(h.state().data.revision, revision);
    button(&mut h, pos, false);
    wait(&mut h, |s| s.press.is_none() && s.pending.is_none());
    let (saved, bodies) = faber_wiring::read(dir.path()).unwrap();
    assert_eq!(saved.revision, revision + 1);
    assert!((volume(&bodies, "left") - 225. * (15. + distance)).abs() < 1e-6);
    assert!((volume(&bodies, "right") - 15_f64.powi(3)).abs() < 1e-6);
}

#[test]
fn typed_distance_moves_the_arrow_and_enter_saves_the_cut() {
    let (dir, mut h) = two_blocks();
    let revision = h.state().data.revision;
    open_on_top_face(&mut h);
    let before = h.state().press_arrow(canvas(&h)).unwrap().0;
    h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Distance")
        .click();
    h.step();
    h.event(egui::Event::Text("-5".into()));
    h.step();
    assert_eq!(h.state().press.as_ref().unwrap().distance, -5.);
    let after = h.state().press_arrow(canvas(&h)).unwrap().0;
    assert!(before.distance(after) > 1.);
    h.key_press(egui::Key::Enter);
    h.step();
    wait(&mut h, |s| s.press.is_none() && s.pending.is_none());
    let (saved, bodies) = faber_wiring::read(dir.path()).unwrap();
    assert_eq!(saved.revision, revision + 1);
    assert!((volume(&bodies, "left") - 2250.).abs() < 1e-6);
}

#[test]
fn a_busy_writer_keeps_the_distance_for_retry() {
    let (dir, mut h) = two_blocks();
    let held = faber_wiring::open(dir.path(), false, None).unwrap();
    open_on_top_face(&mut h);
    let pos = drag_arrow(&mut h, 2.);
    button(&mut h, pos, false);
    wait(&mut h, |s| s.error.is_some() && s.pending.is_none());
    let value = h
        .state()
        .press
        .as_ref()
        .expect("Keep the press pull draft")
        .distance;
    assert!((value - 2.).abs() < 1e-3);
    drop(held);
    h.key_press(egui::Key::Enter);
    h.step();
    wait(&mut h, |s| s.press.is_none() && s.pending.is_none());
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    assert!((volume(&bodies, "left") - 225. * (15. + value)).abs() < 1e-6);
}
