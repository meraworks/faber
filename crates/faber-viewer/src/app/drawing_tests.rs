use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;
fn fixture() -> (tempfile::TempDir, Harness<'static, Shell>) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Drawing test".into())).unwrap();
    let b: faber_model::Part<faber_model::Scalar> = serde_json::from_value(
        json!({"name":"Block","geometry":{"op":"box","size":[40,20,10]},"colour":[100,140,160]}),
    )
    .unwrap();
    p.put_part("block".into(), b).unwrap();
    drop(p);
    std::fs::create_dir_all(dir.path().join("drawings")).unwrap();
    let sheet = json!({"schema":1,"title":"Block elevation","sheet":{"size":"a4","orientation":"landscape"},"projection":"third","scale":1,
        "views":{"front":{"at":[110,110],"kind":"base","direction":"front","parts":["block"],"hidden_lines":true}},
        "dimensions":{},"notes":[],"title_block":{}});
    std::fs::write(dir.path().join("drawings/front.json"), sheet.to_string()).unwrap();
    let (data, bodies) = faber_wiring::read(dir.path()).unwrap();
    let root = dir.path().to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100., 740.))
        .with_step_dt(1. / 60.)
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    for _ in 0..4 {
        h.step();
    }
    (dir, h)
}
fn open(h: &mut Harness<'_, Shell>) {
    h.get_by_role_and_label(egui::Role::Button, "Block elevation")
        .scroll_to_me();
    for _ in 0..6 {
        h.step();
    }
    h.get_by_role_and_label(egui::Role::Button, "Block elevation")
        .click();
    for _ in 0..4 {
        h.step();
    }
}
fn wait(h: &mut Harness<'_, Shell>, label: &str) {
    let start = Instant::now();
    while h.query_all_by_label(label).next().is_none() {
        h.step();
        assert!(start.elapsed() < Duration::from_secs(20), "Missing {label}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn drawing_tab_shows_the_sheet() {
    let (_dir, mut h) = fixture();
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    h.get_by_label("Drawing sheet");
    let page = paper(&h);
    let image = h.render().unwrap();
    let pixel = |x: f32, y: f32| image.get_pixel(x.round() as u32, y.round() as u32).0;
    let white = pixel(page.left() + 3., page.top() + 3.);
    assert!(white[..3].iter().all(|c| *c > 245), "Paper is {white:?}");
    let x = page.left() + 90. / 297. * page.width();
    let y = page.bottom() - 110. / 210. * page.height();
    assert!(
        (-2..=2).any(|dx| pixel(x + dx as f32, y)[0] < 150),
        "Projected edge is missing from the sheet"
    );
    if let Some(path) = std::env::var_os("FABER_DRAWING_TEST_SNAPSHOT") {
        image.save(path).unwrap();
    }
    h.get_by_role_and_label(egui::Role::Button, "Back to model")
        .click();
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Model viewport");
}
#[test]
fn drawing_updates_after_a_model_change() {
    let (dir, mut h) = fixture();
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut next = p.data.parts["block"].clone();
    next.geometry = serde_json::from_value(json!({"op":"box","size":[70,20,10]})).unwrap();
    p.put_part("block".into(), next).unwrap();
    let revision = p.data.revision;
    drop(p);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, &format!("Drawing revision {revision}"));
}

fn paper(h: &Harness<'_, Shell>) -> egui::Rect {
    let canvas = h.get_by_label("Drawing sheet").rect();
    h.state()
        .sheets
        .view
        .as_ref()
        .unwrap()
        .paper_rect(canvas, [297., 210.])
}

fn visible_width(h: &Harness<'_, Shell>) -> f64 {
    use faber_ports::vector::Path2;
    let v = &h
        .state()
        .sheets
        .view
        .as_ref()
        .unwrap()
        .doc
        .as_ref()
        .unwrap()
        .doc;
    let layer = v.layers.iter().find(|l| l.name == "VISIBLE").unwrap();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for path in &layer.paths {
        if let Path2::Polyline { points, .. } = path {
            for p in points {
                lo = lo.min(p[0]);
                hi = hi.max(p[0]);
            }
        }
    }
    hi - lo
}

#[test]
fn drawing_workers_keep_zoom_pan_and_export_controls_still() {
    let (dir, mut h) = fixture();
    h.set_size(egui::vec2(860., 600.));
    for _ in 0..5 {
        h.step();
    }
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    assert!((visible_width(&h) - 40.).abs() < 1e-5);
    let canvas = h.get_by_label("Drawing sheet").rect();
    let button = h
        .get_by_role_and_label(egui::Role::Button, "Fit sheet")
        .rect();
    let spot = canvas.center() + egui::vec2(37., -18.);
    h.hover_at(spot);
    h.step();
    let start = paper(&h);
    let key = (spot - start.min) / start.size();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., 100.),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    });
    for _ in 0..65 {
        h.step();
        let r = paper(&h);
        assert!(
            (r.min + key * r.size() - spot).length() < 0.1,
            "Zoom cursor left the sheet point"
        );
    }
    assert!(paper(&h).width() > start.width() * 1.1);
    let before = paper(&h);
    h.event(egui::Event::PointerButton {
        pos: spot,
        button: egui::PointerButton::Middle,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let shift = egui::vec2(47., 29.);
    h.hover_at(spot + shift);
    h.step();
    h.event(egui::Event::PointerButton {
        pos: spot + shift,
        button: egui::PointerButton::Middle,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    let anchored = paper(&h);
    assert!(
        (anchored.min - before.min - shift).length() < 1.,
        "Pan did not follow the pointer"
    );
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut block = project.data.parts["block"].clone();
    block.geometry = serde_json::from_value(json!({"op":"box","size":[54,20,10]})).unwrap();
    project.put_part("block".into(), block).unwrap();
    let rev = project.data.revision;
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    let due = Instant::now() + Duration::from_secs(20);
    let mut idle = 0;
    while idle < 8 {
        h.step();
        assert_eq!(canvas, h.get_by_label("Drawing sheet").rect());
        assert_eq!(
            button,
            h.get_by_role_and_label(egui::Role::Button, "Fit sheet")
                .rect()
        );
        assert!((paper(&h).center() - anchored.center()).length() < 0.1);
        assert!((paper(&h).width() - anchored.width()).abs() < 0.1);
        if h.query_all_by_label(&format!("Drawing revision {rev}"))
            .next()
            .is_some()
        {
            idle += 1;
        }
        assert!(Instant::now() < due);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!((visible_width(&h) - 54.).abs() < 1e-5);
    h.get_by_role_and_label(egui::Role::Button, "Export")
        .click();
    let path = dir.path().join("front.pdf");
    while !path.is_file() {
        h.step();
        assert!(Instant::now() < due);
        assert_eq!(canvas, h.get_by_label("Drawing sheet").rect());
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(std::fs::read(path).unwrap().starts_with(b"%PDF-1.4"));
}

#[test]
fn drawing_reloads_while_the_mcp_writer_keeps_the_project_open() {
    let (dir, mut h) = fixture();
    let mut writer = faber_wiring::open(dir.path(), false, None).unwrap();
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    let mut sheet = writer.data.drawings["front"].clone();
    sheet.title = "Revised elevation".into();
    sheet.sheet.size = faber_model::drawing::SheetSize::A3;
    let revision = writer.data.revision;
    let old = h.state_mut().sheets.hold_render(revision);
    let export_rect = h.get_by_role_and_label(egui::Role::Button, "Export").rect();
    writer
        .batch_at_revision(
            revision,
            vec![faber_model::Edit::SetDrawing {
                id: "front".into(),
                drawing: sheet.clone(),
            }],
        )
        .unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    wait(&mut h, "Revised elevation");
    h.get_by_label("Updating drawing…");
    h.get_by_label("A4 · 297 × 210 mm");
    let button = h.get_by_role_and_label(egui::Role::Button, "Export");
    assert!(button.accesskit_node().is_disabled());
    assert_eq!(
        export_rect,
        button.rect(),
        "Export moved while the previous revision was visible"
    );
    old.send(Err("An old drawing failed.".into())).unwrap();
    wait(&mut h, "A3 · 420 × 297 mm");
    assert!(
        h.query_all_by_label("An old drawing failed.")
            .next()
            .is_none()
    );
    assert!(
        !h.get_by_role_and_label(egui::Role::Button, "Export")
            .accesskit_node()
            .is_disabled()
    );
    let old = h.state_mut().sheets.hold_render(writer.data.revision);
    writer
        .batch_at_revision(
            writer.data.revision,
            vec![
                faber_model::Edit::RemoveDrawing { id: "front".into() },
                faber_model::Edit::SetDrawing {
                    id: "elevation".into(),
                    drawing: sheet,
                },
            ],
        )
        .unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    wait(&mut h, "Model viewport");
    old.send(Err("Removed drawing failed.".into())).unwrap();
    h.get_by_role_and_label(egui::Role::Button, "Revised elevation")
        .click();
    wait(&mut h, "A3 · 420 × 297 mm");
    assert!(
        h.query_all_by_label("Removed drawing failed.")
            .next()
            .is_none()
    );
    h.get_by_role_and_label(egui::Role::Button, "Export")
        .click();
    let output = dir.path().join("elevation.pdf");
    wait(&mut h, &format!("Exported {}.", output.display()));
    assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF"));
    assert!(!dir.path().join("front.pdf").exists());
    // The sheet can change between a click and the next file poll.
    h.state_mut().last_poll = Instant::now() + Duration::from_secs(30);
    writer
        .batch_at_revision(
            writer.data.revision,
            vec![faber_model::Edit::RemoveDrawing {
                id: "elevation".into(),
            }],
        )
        .unwrap();
    std::fs::write(&output, b"Keep the previous export").unwrap();
    h.get_by_role_and_label(egui::Role::Button, "Export")
        .click();
    wait(
        &mut h,
        "The project changed. Read the current drawing revision.",
    );
    assert_eq!(std::fs::read(output).unwrap(), b"Keep the previous export");
}

#[test]
fn drawing_export_controls_keep_their_place_during_loading_and_error_frames() {
    let (_dir, mut h) = fixture();
    h.set_size(egui::vec2(650., 390.));
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    let button =
        |h: &Harness<'_, Shell>| h.get_by_role_and_label(egui::Role::Button, "Export").rect();
    let spot = button(&h);
    let canvas = h.get_by_label("Drawing sheet").rect();
    let revision = h.state().data.revision;
    let held = h.state_mut().sheets.hold_render(revision);
    for _ in 0..4 {
        h.step();
        assert_eq!(
            button(&h),
            spot,
            "The export button moved when loading began"
        );
        assert_eq!(h.get_by_label("Drawing sheet").rect(), canvas);
    }
    let error = "The drawing source has changed. Select Retry drawing to load the new sheet.";
    held.send(Err(error.into())).unwrap();
    wait(&mut h, error);
    for _ in 0..6 {
        h.step();
        assert_eq!(
            button(&h),
            spot,
            "The export button moved when a render failed"
        );
    }
    let retry = h.get_by_role_and_label(egui::Role::Button, "Retry drawing");
    retry.scroll_to_me();
    for _ in 0..20 {
        h.step();
    }
    let r = h
        .get_by_role_and_label(egui::Role::Button, "Retry drawing")
        .rect();
    assert!(r.bottom() <= 390. && r.top() >= 0.);
    assert!(r.top() >= h.get_by_label(error).rect().bottom());
    h.get_by_role_and_label(egui::Role::Button, "Retry drawing")
        .click();
    let due = Instant::now() + Duration::from_secs(6);
    while h.query_all_by_label(error).next().is_some()
        || h.query_all_by_label("Updating drawing…").next().is_some()
    {
        h.step();
        assert!(Instant::now() < due);
        std::thread::sleep(Duration::from_millis(4));
    }
}

#[test]
fn drawing_tracks_same_revision_edits_to_sheet_and_part_files() {
    let (dir, mut h) = fixture();
    open(&mut h);
    wait(&mut h, "A4 · 297 × 210 mm");
    let revision = h.state().data.revision;
    let late = h.state_mut().sheets.hold_render(revision);
    let file = dir.path().join("parts/block.json");
    let mut part: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    part["geometry"]["size"][0] = json!(73.);
    std::fs::write(file, serde_json::to_vec_pretty(&part).unwrap()).unwrap();
    let file = dir.path().join("drawings/front.json");
    let mut drawing: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    drawing["title"] = json!("Edited by hand");
    std::fs::write(file, serde_json::to_vec_pretty(&drawing).unwrap()).unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    wait(&mut h, "Edited by hand");
    assert_eq!(h.state().data.revision, revision);
    late.send(Err("Discard this old page.".into())).unwrap();
    let due = Instant::now() + Duration::from_secs(5);
    while (visible_width(&h) - 73.).abs() > 1e-5 {
        h.step();
        assert!(
            h.query_all_by_label("Discard this old page.")
                .next()
                .is_none(),
            "A worker updated a page from the previous file generation"
        );
        assert!(
            Instant::now() < due,
            "The old drawing still measures {} mm",
            visible_width(&h)
        );
        std::thread::sleep(Duration::from_millis(4));
    }
    h.get_by_role_and_label(egui::Role::Button, "Export")
        .click();
    let output = dir.path().join("front.pdf");
    wait(&mut h, &format!("Exported {}.", output.display()));
    assert!(std::fs::read(output).unwrap().starts_with(b"%PDF"));
}
