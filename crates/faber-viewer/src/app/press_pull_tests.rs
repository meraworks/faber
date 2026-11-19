use super::controls_fixture::{canvas, centre, click, two_blocks};
use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    for item in std::fs::read_dir(dir).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else if path.file_name().is_some_and(|name| {
            // The viewer link files are display state, not project data.
            name == faber_wiring::viewer_link::STATE_FILE
                || name == faber_wiring::viewer_link::COMMAND_FILE
        }) {
            continue;
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
fn open_face_menu(h: &mut Harness<'_, Shell>) {
    h.get_by_label("Face").click();
    h.step();
    let p = centre(h, "left");
    click(h, p, egui::PointerButton::Secondary, egui::Modifiers::NONE);
    for _ in 0..4 {
        h.step();
    }
    assert!(matches!(h.state().picks.last(),Some(Pick::Face{part,..}) if part=="left"));
}
#[test]
fn face_menu_has_the_four_items() {
    let (_dir, mut h) = two_blocks();
    open_face_menu(&mut h);
    for label in ["Remove face", "Move face", "Split face", "Emboss"] {
        h.get_by_role_and_label(egui::Role::Button, label);
    }
}
#[test]
fn move_face_drag_previews_and_saves_one_feature() {
    let (dir, mut h) = two_blocks();
    let original = files(dir.path());
    let revision = h.state().data.revision;
    open_face_menu(&mut h);
    h.get_by_role_and_label(egui::Role::Button, "Move face")
        .click();
    h.step();
    wait(&mut h, |s| {
        s.press.as_ref().is_some_and(|p| p.face.is_some())
    });
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.state_mut().dirty = true;
    h.step();
    let view = canvas(&h);
    let camera = h.state().camera.matrix(view.aspect_ratio());
    let pos = drag_arrow(&mut h, 2.);
    wait(&mut h, |s| {
        s.press
            .as_ref()
            .and_then(|t| t.preview.as_ref())
            .is_some_and(|b| (volume(b, "left") - 3825.).abs() < 0.05)
    });
    assert_eq!(files(dir.path()), original);
    assert_eq!(canvas(&h), view);
    assert_eq!(h.state().camera.matrix(view.aspect_ratio()), camera);
    button(&mut h, pos, false);
    wait(&mut h, |s| s.press.is_none() && s.pending.is_none());
    let (d, b) = faber_wiring::read(dir.path()).unwrap();
    assert_eq!(d.revision, revision + 1);
    let recipe = serde_json::to_value(&d.parts["left"].geometry).unwrap();
    let fs = recipe["features"].as_array().unwrap();
    assert_eq!(
        fs.len(),
        2,
        "One face operation must follow the original box"
    );
    assert_eq!(fs[1]["geometry"]["op"], "move_faces");
    assert!((volume(&b, "left") - 3825.).abs() < 0.05);
    h.get_by_role_and_label(egui::Role::Button, "Undo").click();
    h.step();
    wait(&mut h, |s| {
        s.pending.is_none() && (volume(&s.bodies, "left") - 3375.).abs() < 1e-6
    });
}
#[test]
fn move_face_rejects_typed_through_distance_and_cancel_restores_the_view() {
    let (dir, mut h) = two_blocks();
    let original = files(dir.path());
    open_face_menu(&mut h);
    h.get_by_role_and_label(egui::Role::Button, "Move face")
        .click();
    h.step();
    wait(&mut h, |s| {
        s.press.as_ref().is_some_and(|p| p.face.is_some())
    });
    let cancel = h.get_by_role_and_label(egui::Role::Button, "Cancel").rect();
    h.get_by_role_and_label(egui::Role::SpinButton, "Distance")
        .click();
    h.step();
    h.event(egui::Event::Text("-25".into()));
    h.step();
    let due = Instant::now() + Duration::from_secs(12);
    while h
        .query_all_by_label_contains("Distance -25 mm removes the body")
        .next()
        .is_none()
    {
        assert!(Instant::now() < due);
        std::thread::sleep(Duration::from_millis(3));
        h.step();
    }
    assert_eq!(
        h.get_by_role_and_label(egui::Role::Button, "Cancel").rect(),
        cancel,
        "The error moved the face controls."
    );
    h.get_by_role_and_label(egui::Role::Button, "Cancel")
        .click();
    h.step();
    h.step();
    assert!(h.state().press.is_none());
    assert_eq!(files(dir.path()), original);
}
#[test]
fn face_forms_keep_the_pick_and_commit_or_refuse_the_selected_operation() {
    use serde_json::json;
    let (dir, mut h) = two_blocks();
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut p = serde_json::to_value(&project.data.parts["left"]).unwrap();
    let rows = p["geometry"]["features"].as_array_mut().unwrap();
    rows.insert(0,json!({"id":"middle","geometry":{"op":"plane","placement":{"translation":[7.5,0,0],"rotation":[0,90,0]}}}));
    rows.insert(1,json!({"id":"top_reference","geometry":{"op":"plane","placement":{"translation":[0,0,15],"rotation":[0,0,0]}}}));
    rows.insert(2,json!({"id":"mark","geometry":{"op":"sketch_on_plane","plane":"top_reference","profile":{"outer":{"kind":"circle","centre":[7.5,7.5],"radius":2},"holes":[]}}}));
    project
        .put_part(
            "left".into(),
            serde_json::from_value::<faber_model::Part<faber_model::Scalar>>(p).unwrap(),
        )
        .unwrap();
    let revision = project.data.revision;
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    wait(&mut h, |s| s.data.revision == revision);
    open_face_menu(&mut h);
    h.get_by_role_and_label(egui::Role::Button, "Remove face")
        .click();
    h.step();
    h.step();
    h.get_by_role_and_label(egui::Role::Button, "Apply feature")
        .click();
    h.step();
    wait(&mut h, |s| s.window.error.is_some() || s.error.is_some());
    assert_eq!(faber_wiring::read(dir.path()).unwrap().0.revision, revision);
    h.get_by_role_and_label(egui::Role::Button, "Cancel feature")
        .click();
    h.step();
    h.step();
    for (i, label) in ["Split face", "Emboss"].into_iter().enumerate() {
        let before = h.state().data.revision;
        let pos = centre(&h, "left");
        click(
            &mut h,
            pos,
            egui::PointerButton::Secondary,
            egui::Modifiers::NONE,
        );
        for _ in 0..3 {
            h.step();
        }
        let action = h.get_by_role_and_label(egui::Role::Button, label);
        let rect = action.rect();
        assert!(
            !action.accesskit_node().is_disabled(),
            "{label} is disabled."
        );
        assert!(
            h.state().marking.context_clip.unwrap().contains_rect(rect),
            "{label} at {rect:?} is clipped by the context menu."
        );
        h.get_by_role_and_label(egui::Role::Button, label).click();
        for _ in 0..3 {
            h.step();
        }
        assert!(
            h.state().feature_draft.is_some(),
            "{label} did not open its feature form."
        );
        if i == 0 {
            h.get_by_role_and_label(egui::Role::ComboBox, "Splitting tool")
                .click();
            for _ in 0..4 {
                h.step();
            }
            h.get_by_label("Plane · middle");
            h.get_by_role_and_label(egui::Role::Button, "Sketch · mark")
                .click();
            for _ in 0..3 {
                h.step();
            }
        } else {
            h.get_by_role_and_label(egui::Role::ComboBox, "Profile sketch");
        }
        h.get_by_role_and_label(egui::Role::Button, "Apply feature")
            .click();
        h.step();
        wait(&mut h, |s| s.data.revision > before);
        let (data, b) = faber_wiring::read(dir.path()).unwrap();
        let value = serde_json::to_value(&data.parts["left"].geometry).unwrap();
        let op = &value["features"].as_array().unwrap().last().unwrap()["geometry"];
        assert_eq!(op["op"], if i == 0 { "split_face" } else { "emboss" });
        if i == 0 {
            assert_eq!(op["tool"], json!({"kind":"sketch","sketch":"mark"}));
            assert!((volume(&b, "left") - 3375.).abs() < 1e-6);
            h.get_by_role_and_label(egui::Role::Button, "Undo").click();
            h.step();
            wait(&mut h, |s| {
                s.pending.is_none() && s.data.revision > revision + 1
            });
        } else {
            assert!(volume(&b, "left") > 3375.);
        }
    }
}
#[test]
fn face_actions_are_available_from_the_default_canvas_menu() {
    let (_dir, mut h) = two_blocks();
    assert!(h.state().select_filter == SelectFilter::Body);
    let point = centre(&h, "left");
    click(
        &mut h,
        point,
        egui::PointerButton::Secondary,
        egui::Modifiers::NONE,
    );
    h.get_by_role_and_label(egui::Role::Button, "Move/Copy M");
    wait(&mut h, |s| s.cached_pick_topology("left").is_some());
    h.step();
    for label in ["Split face", "Emboss"] {
        assert!(
            !h.get_by_role_and_label(egui::Role::Button, label)
                .accesskit_node()
                .is_disabled(),
            "The face under the pointer must enable {label} in body mode."
        );
    }
    h.get_by_role_and_label(egui::Role::Button, "Move face")
        .click();
    h.step();
    h.step();
    wait(&mut h, |s| {
        s.press.as_ref().is_some_and(|p| p.face.is_some())
    });
    assert!(matches!(h.state().picks.as_slice(),[Pick::Face{part,..}] if part=="left"));
    h.key_press(egui::Key::Escape);
    h.step();
    assert!(h.state().press.is_none());
}
