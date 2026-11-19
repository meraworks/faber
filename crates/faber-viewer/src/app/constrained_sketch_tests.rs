use super::controls_fixture::{click, two_blocks};
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

fn button(h: &mut Harness<'_, Shell>, label: &str) {
    h.get_by_label(label).click();
    for _ in 0..4 {
        h.step();
    }
}
fn new_editor(h: &mut Harness<'_, Shell>) {
    button(h, "New sketch");
    button(h, "Constrained sketch");
}
fn at(h: &Harness<'_, Shell>, dx: f32, dy: f32) -> egui::Pos2 {
    h.get_by_label("Constraint canvas").rect().center() + egui::vec2(dx, dy)
}
fn draw(h: &mut Harness<'_, Shell>, positions: &[(f32, f32)]) {
    for &(dx, dy) in positions {
        let pos = at(h, dx, dy);
        click(h, pos, egui::PointerButton::Primary, egui::Modifiers::NONE);
    }
}
fn rectangle(h: &mut Harness<'_, Shell>) {
    new_editor(h);
    button(h, "Rectangle");
    draw(h, &[(-80., 40.), (80., -40.)]);
}
fn saved(h: &mut Harness<'_, Shell>, revision: u64) {
    let start = Instant::now();
    while h.state().data.revision != revision {
        assert!(
            start.elapsed() < Duration::from_secs(12),
            "{:?}",
            h.state().window.error
        );
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn sketch_mode_shows_the_degree_count() {
    let (_dir, mut h) = two_blocks();
    new_editor(&mut h);
    button(&mut h, "Line");
    draw(&mut h, &[(-30., 30.), (40., -10.)]);
    h.get_by_label("4 degrees of freedom");
    button(&mut h, "Constraints");
    for name in [
        "Coincident",
        "Point on line",
        "Point on circle",
        "Midpoint",
        "Horizontal",
        "Vertical",
        "Parallel",
        "Perpendicular",
        "Collinear",
        "Tangent",
        "Equal",
        "Concentric",
        "Symmetric",
        "Fix",
        "Distance",
        "Horizontal distance",
        "Vertical distance",
        "Point to line distance",
        "Angle",
        "Radius",
        "Diameter",
        "Length",
    ] {
        h.get_by_label(name);
    }
}

#[test]
fn dragging_a_free_point_keeps_the_width() {
    let (_dir, mut h) = two_blocks();
    rectangle(&mut h);
    h.get_by_label("Fully constrained · 0 degrees of freedom");
    button(&mut h, "Remove constraint 7");
    h.get_by_label("1 degree of freedom");
    let p = h.get_by_label("Point c").rect().center();
    let before = h
        .get_by_label("Point b")
        .rect()
        .center()
        .distance(h.get_by_label("Point a").rect().center());
    h.hover_at(p);
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let target = p + egui::vec2(12., -28.);
    h.event(egui::Event::PointerMoved(target));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let after = h
        .get_by_label("Point b")
        .rect()
        .center()
        .distance(h.get_by_label("Point a").rect().center());
    assert!((after - before).abs() < 0.02);
    assert!(h.get_by_label("Point c").rect().center().y < p.y - 15.);
}

#[test]
fn editing_a_dimension_rebuilds_the_part() {
    let (dir, mut h) = two_blocks();
    let revision = h.state().data.revision;
    rectangle(&mut h);
    button(&mut h, "Finish");
    saved(&mut h, revision + 1);
    let id = h
        .state()
        .data
        .parts
        .keys()
        .find(|id| id.starts_with("body_"))
        .unwrap()
        .clone();
    let initial = h.state().bodies.iter().find(|b| b.id == id).unwrap().volume;
    h.state_mut().selected = Some(id.clone());
    h.step();
    button(&mut h, "Edit sketch");
    let dim = h.get_by_label("Dimension 6").rect().center();
    for _ in 0..2 {
        click(
            &mut h,
            dim,
            egui::PointerButton::Primary,
            egui::Modifiers::NONE,
        );
    }
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .type_text("25 + 25");
    h.step();
    button(&mut h, "Set dimension");
    assert_eq!(h.state().data.revision, revision + 1);
    let fixed_at = h.get_by_label("Point b").rect();
    button(&mut h, "Point b");
    button(&mut h, "Constraints");
    button(&mut h, "Fix");
    assert_eq!(h.get_by_label("Point b").rect(), fixed_at);
    assert!(
        !h.get_by_label("Finish").accesskit_node().is_disabled(),
        "Fix must anchor the displayed point."
    );
    button(&mut h, "Finish");
    saved(&mut h, revision + 2);
    let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
    let wire = serde_json::to_value(&data.parts[&id].geometry).unwrap();
    assert_eq!(
        wire["features"][1]["geometry"]["sketch"]["constraints"][5]["value"],
        "25 + 25"
    );
    assert_ne!(bodies.iter().find(|b| b.id == id).unwrap().volume, initial);
    button(&mut h, "Undo");
    saved(&mut h, revision + 3);
    assert!((h.state().bodies.iter().find(|b| b.id == id).unwrap().volume - initial).abs() < 1e-6);
}

#[test]
fn conflict_is_shown_and_finish_is_disabled() {
    let (_dir, mut h) = two_blocks();
    rectangle(&mut h);
    button(&mut h, "Point a");
    button(&mut h, "Point b");
    button(&mut h, "Constraints");
    button(&mut h, "Horizontal distance");
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .type_text("53");
    h.step();
    button(&mut h, "Set dimension");
    h.get_by_label("Conflicting constraints");
    assert!(h.get_by_label("Finish").accesskit_node().is_disabled());
}

#[test]
fn invalid_dimension_preserves_the_last_visible_sketch() {
    let (_dir, mut h) = two_blocks();
    rectangle(&mut h);
    let before = h.get_by_label("Point c").rect();
    let dim = h.get_by_label("Dimension 6").rect().center();
    for _ in 0..2 {
        click(
            &mut h,
            dim,
            egui::PointerButton::Primary,
            egui::Modifiers::NONE,
        );
    }
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(egui::Role::TextInput, "Dimension value")
        .type_text("no_such_parameter");
    h.step();
    button(&mut h, "Set dimension");
    h.get_by_label("Unknown parameter: no_such_parameter");
    assert_eq!(h.get_by_label("Point c").rect(), before);
    assert!(h.get_by_label("Finish").accesskit_node().is_disabled());
}

#[test]
fn offset_sketch_commits_a_named_profile_and_keeps_the_part_output() {
    let (dir, mut h) = two_blocks();
    let revision = h.state().data.revision;
    rectangle(&mut h);
    button(&mut h, "Finish");
    saved(&mut h, revision + 1);
    let id = h
        .state()
        .data
        .parts
        .keys()
        .find(|id| id.starts_with("body_"))
        .unwrap()
        .clone();
    h.state_mut().selected = Some(id.clone());
    h.step();
    button(&mut h, "Offset sketch");
    h.get_by_label("Profile sketch");
    h.get_by_role_and_label(egui::Role::TextInput, "distance")
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(egui::Role::TextInput, "distance")
        .type_text("-2");
    h.step();
    button(&mut h, "Apply feature");
    saved(&mut h, revision + 2);
    let (data, _) = faber_wiring::read(dir.path()).unwrap();
    let g = serde_json::to_value(&data.parts[&id].geometry).unwrap();
    assert_eq!(g["output"], "extrude");
    let profile = g["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["geometry"]["op"] == "offset_sketch")
        .unwrap();
    assert_eq!(profile["geometry"]["sketch"], "sketch");
    assert_eq!(profile["geometry"]["distance"], -2.);
}

#[test]
fn dragging_the_free_coordinate_of_a_parameter_point_survives_reopening() {
    use faber_model::{Edit, Part, Scalar};
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Parameter point".into())).unwrap();
    let part:Part<Scalar>=serde_json::from_value(serde_json::json!({
        "name":"Circle","colour":[95,140,170],"geometry":{"op":"history","output":"extrude","features":[
            {"id":"plane","geometry":{"op":"plane","placement":{"translation":[0,0,0],"rotation":[0,0,0]}}},
            {"id":"sketch","geometry":{"op":"solved_sketch","plane":"plane","sketch":{"points":{"p":["width",10]},"entities":{"hole":{"kind":"circle","centre":"p","radius":3}}}}},
            {"id":"extrude","geometry":{"op":"extrude_sketch","source":"sketch","vector":[0,0,4]}}
        ]}
    })).unwrap();
    p.batch(vec![
        Edit::SetParam {
            name: "width".into(),
            expression: "20".into(),
        },
        Edit::PutPart {
            id: "plate".into(),
            part,
        },
    ])
    .unwrap();
    let rev = p.data.revision;
    let data = p.data.clone();
    let bodies = p.bodies.clone();
    drop(p);
    let root = dir.path().to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 900.))
        .with_step_dt(1. / 60.)
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    h.state_mut().selected = Some("plate".into());
    h.step();
    button(&mut h, "Edit sketch");
    let from = h.get_by_label("Point p").rect().center();
    let to = from + egui::vec2(24., -32.);
    h.hover_at(from);
    h.step();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    h.hover_at(to);
    h.step();
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let after = h.get_by_label("Point p").rect().center();
    assert!(
        (after.x - from.x).abs() < 0.01 && (after.y - to.y).abs() < 0.01,
        "Point moved from {from:?} to {after:?} instead of the free-axis target."
    );
    button(&mut h, "Finish");
    saved(&mut h, rev + 1);
    let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
    let g = serde_json::to_value(&data.parts["plate"].geometry).unwrap();
    let point = &g["features"][1]["geometry"]["sketch"]["points"]["p"];
    assert_eq!(point[0], "width");
    assert!((point[1].as_f64().unwrap() - 14.).abs() < 1e-6);
    let mesh_centre =
        (glam::DVec3::from(bodies[0].bounds[0]) + glam::DVec3::from(bodies[0].bounds[1])) / 2.;
    assert!((mesh_centre.x - 20.).abs() < 1e-5 && (mesh_centre.y - 14.).abs() < 1e-5);
}
