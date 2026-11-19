use super::controls_fixture::{canvas, click, two_blocks};
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

fn press(h: &mut Harness<'_, Shell>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    for _ in 0..6 {
        h.step();
    }
    until(h, |h| !h.get_by_label(label).accesskit_node().is_disabled());
    h.get_by_label(label).click();
    for _ in 0..3 {
        h.step();
    }
}

fn text(h: &mut Harness<'_, Shell>, label: &str, value: &str) {
    let field = h.get_by_role_and_label(egui::Role::TextInput, label);
    field.scroll_to_me();
    for _ in 0..5 {
        h.step();
    }
    h.get_by_role_and_label(egui::Role::TextInput, label)
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(egui::Role::TextInput, label)
        .type_text(value);
    h.step();
}

fn until(h: &mut Harness<'_, Shell>, condition: impl Fn(&Harness<'_, Shell>) -> bool) {
    let start = Instant::now();
    while !condition(h) {
        h.step();
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Sheet state did not update: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}

fn fixture() -> (tempfile::TempDir, Harness<'static, Shell>) {
    let (dir, mut h) = two_blocks();
    press(&mut h, "New sheet metal");
    press(&mut h, "Create sheet");
    until(&mut h, |h| h.state().data.parts.contains_key("sheet_1"));
    h.state_mut().fit();
    h.state_mut().camera.set_view(faber_scene::View::Top);
    h.step();
    (dir, h)
}

fn flange(h: &mut Harness<'_, Shell>) {
    press(h, "Add flange");
    until(h, |h| {
        h.query_by_label("Pick a straight wall edge").is_some()
    });
    let viewport = canvas(h);
    let camera = h.state().camera.matrix(viewport.aspect_ratio());
    let other = move_tool::screen_point(viewport, camera, glam::dvec3(-30., 7.5, 15.)).unwrap();
    click(
        h,
        other,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("Select a free straight edge of this sheet.");
    let pos = move_tool::screen_point(viewport, camera, glam::dvec3(50., 20., 2.)).unwrap();
    click(h, pos, egui::PointerButton::Primary, egui::Modifiers::NONE);
    until(h, |h| h.query_by_label("base · edge 2").is_some());
    let rev = h.state().data.revision;
    press(h, "Save flange");
    until(h, |h| h.state().data.revision == rev + 1);
    assert_eq!(camera, h.state().camera.matrix(viewport.aspect_ratio()));
}

#[test]
fn add_flange_by_edge_click() {
    let (dir, mut h) = fixture();
    flange(&mut h);
    let project = faber_wiring::read_snapshot(dir.path()).unwrap();
    let b = project.bodies.iter().find(|b| b.id == "sheet_1").unwrap();
    for (actual, expected) in b.bounds[1].iter().zip([54., 40., 34.]) {
        assert!((actual - expected).abs() < 1e-5, "{:?}", b.bounds);
    }
}

#[test]
fn rule_change_rebuilds_the_part() {
    let (dir, mut h) = fixture();
    let rev = h.state().data.revision;
    press(&mut h, "Edit sheet rule");
    text(&mut h, "Thickness (mm)", "1 + 2");
    press(&mut h, "Save rule");
    until(&mut h, |h| h.state().data.revision == rev + 1);
    let project = faber_wiring::read_snapshot(dir.path()).unwrap();
    let b = project.bodies.iter().find(|b| b.id == "sheet_1").unwrap();
    assert!((b.bounds[1][2] - 3.).abs() < 1e-6);
    assert!(
        serde_json::to_string(&project.data.parts["sheet_1"])
            .unwrap()
            .contains("1 + 2")
    );
}

#[test]
fn flat_pattern_switch_shows_the_bend_lines() {
    let (dir, mut h) = fixture();
    flange(&mut h);
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.state_mut().dirty = true;
    for _ in 0..4 {
        h.step();
        assert!(
            h.query_by_label("The sheet changed. Read it again.")
                .is_none(),
            "Saving the flange in this viewer displayed a project conflict."
        );
    }
    let folded = camera_pose(&h);
    let captures = std::env::var_os("FABER_SHEET_TEST_CAPTURE").map(PathBuf::from);
    if let Some(p) = &captures {
        std::fs::create_dir_all(p).unwrap();
        h.render().unwrap().save(p.join("folded.png")).unwrap();
    }
    press(&mut h, "Flat pattern");
    until(&mut h, |h| {
        h.query_by_label("Flat pattern viewport").is_some()
    });
    until(&mut h, |h| h.query_by_label("84.52 × 40.00 mm").is_some());
    let viewport = h.get_by_label("Flat pattern viewport").rect();
    let export = h.get_by_label("Export flat pattern").rect();
    for _ in 0..8 {
        h.step();
        assert_eq!(h.get_by_label("Flat pattern viewport").rect(), viewport);
        assert_eq!(h.get_by_label("Export flat pattern").rect(), export);
    }
    let before = h.render().unwrap();
    if let Some(p) = &captures {
        before.save(p.join("flat.png")).unwrap();
    }
    press(&mut h, "Show bend lines");
    let after = h.render().unwrap();
    let changed = (viewport.top() as u32 + 3..viewport.bottom() as u32 - 3)
        .flat_map(|y| {
            (viewport.left() as u32 + 3..viewport.right() as u32 - 3).map(move |x| (x, y))
        })
        .filter(|&(x, y)| before.get_pixel(x, y) != after.get_pixel(x, y))
        .count();
    assert!(changed > 20, "No bend pixels changed: {changed}");
    for ext in ["dxf", "svg"] {
        let file = dir.path().join(format!("pattern.{ext}"));
        text(&mut h, "Flat pattern file", file.to_str().unwrap());
        press(&mut h, "Export flat pattern");
        until(&mut h, |_| file.is_file());
        let saved = std::fs::read_to_string(file).unwrap();
        assert!(saved.contains("CUT") && saved.contains("BEND"));
    }
    press(&mut h, "Flat pattern");
    assert!(h.query_by_label("Model viewport").is_some());
    assert_eq!(camera_pose(&h), folded);
}

fn camera_pose(h: &Harness<'_, Shell>) -> glam::Mat4 {
    h.state().camera.matrix(1.)
}

#[test]
fn generated_sheet_explains_source_ownership_and_keeps_export_available() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    std::fs::write(
        root.join("design.jsonnet"),
        include_str!("../../../../examples/sheet-bracket/design.jsonnet"),
    )
    .unwrap();
    faber_wiring::design_source().build(&root).unwrap();
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 900.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    h.state_mut().select_only(Some("bracket".into()));
    h.step();
    press(&mut h, "Sheet metal");
    for btn in [
        "New sheet metal",
        "Edit sheet rule",
        "Add flange",
        "Edit mount",
    ] {
        assert!(
            h.get_by_label(btn).accesskit_node().is_disabled(),
            "{btn} cannot save this generated part"
        );
    }
    h.get_by_label("Edit the sheet in design.jsonnet, then build the project.");
    press(&mut h, "Flat pattern");
    until(&mut h, |h| {
        !h.get_by_label("Export flat pattern")
            .accesskit_node()
            .is_disabled()
    });
    let path = dir.path().join("generated.svg");
    text(&mut h, "Flat pattern file", path.to_str().unwrap());
    press(&mut h, "Export flat pattern");
    until(&mut h, |_| path.exists());
    assert_eq!(
        std::fs::read_to_string(path)
            .unwrap()
            .matches("<circle ")
            .count(),
        3
    );
}

#[test]
fn flat_pattern_remains_visible_during_a_real_reload_and_rejects_stale_export() {
    let (dir, mut h) = fixture();
    flange(&mut h);
    press(&mut h, "Flat pattern");
    until(&mut h, |h| {
        !h.get_by_label("Export flat pattern")
            .accesskit_node()
            .is_disabled()
    });
    let size = "84.52 × 40.00 mm";
    let width = h.get_by_label(size).rect();
    let before = h.render().unwrap();
    let canvas = h.get_by_label("Flat pattern viewport").rect();
    let camera = camera_pose(&h);
    let (send, recv) = mpsc::channel();
    h.state_mut().sheet_metal.as_mut().unwrap().read_gate = Some(recv);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut other = project.data.parts["left"].clone();
    other.name = "Renamed block".into();
    project.put_part("left".into(), other).unwrap();
    let rev = project.data.revision;
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    until(&mut h, |h| h.state().data.revision == rev);
    for _ in 0..8 {
        h.step();
        assert_eq!(h.get_by_label(size).rect(), width);
        assert!(
            h.get_by_label("Export flat pattern")
                .accesskit_node()
                .is_disabled()
        );
        assert_eq!(h.get_by_label("Flat pattern viewport").rect(), canvas);
        assert_eq!(camera_pose(&h), camera);
    }
    let stale = h.render().unwrap();
    for y in canvas.top() as u32 + 4..canvas.bottom() as u32 - 4 {
        for x in canvas.left() as u32 + 4..canvas.right() as u32 - 4 {
            assert_eq!(
                before.get_pixel(x, y),
                stale.get_pixel(x, y),
                "The pattern changed at {x}, {y}."
            );
        }
    }
    // Disconnect the worker before it publishes this revision.
    drop(send);
    until(&mut h, |h| {
        h.query_by_label("The sheet worker stopped.").is_some()
    });
    assert!(
        h.get_by_label("Export flat pattern")
            .accesskit_node()
            .is_disabled()
    );
    assert!(h.get_by_label("Add flange").accesskit_node().is_disabled());
    h.get_by_label(size);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    project.remove_part("sheet_1").unwrap();
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    until(&mut h, |h| {
        h.query_by_label("Flat pattern viewport").is_none()
    });
    h.get_by_label("Model viewport");
}
