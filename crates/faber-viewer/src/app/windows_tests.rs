use super::*;
use egui_kittest::kittest::{NodeT, Queryable};

type Harness = egui_kittest::Harness<'static, Shell>;

fn fixture(width: f32, height: f32) -> (tempfile::TempDir, Harness) {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Windows".into())).unwrap();
    project
        .set_params([("width".into(), "20".into())].into())
        .unwrap();
    let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
        "name":"Box", "colour":[120,140,160], "geometry":{"op":"history",
        "features":[{"id":"first","geometry":{"op":"box","size":["width",10,10]}},
                    {"id":"second","geometry":{"op":"box","size":[8,12,10]}}],
        "output":"second"}
    }))
    .unwrap();
    project.put_part("box".into(), part).unwrap();
    let data = project.data.clone();
    let bodies = project.bodies.clone();
    drop(project);
    let root = dir.path().to_owned();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(width, height))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    harness.state_mut().select_only(Some("box".into()));
    settle(&mut harness);
    (dir, harness)
}

fn settle(h: &mut Harness) {
    for _ in 0..4 {
        h.step();
    }
}

fn open(h: &mut Harness, kind: ToolWindow) {
    let shell = h.state_mut();
    match kind {
        ToolWindow::CreateProject => shell.request_window(Draft::Project(ProjectDraft {
            create: true,
            path: String::new(),
            name: "Design".into(),
        })),
        ToolWindow::MoveRotate => shell.start_placement("box"),
        ToolWindow::MoveCopy => shell.start_move("box"),
        ToolWindow::Export => shell.request_window(Draft::Export),
        ToolWindow::Checks => shell.request_window(Draft::Checks),
        ToolWindow::Parameters => shell.request_window(Draft::Parameters(
            shell.data.clone(),
            shell.data.params.clone(),
        )),
        ToolWindow::Feature => shell.new_feature("box", ToolKind::Shell).unwrap(),
        ToolWindow::Sketch => shell.request_window(Draft::Sketch(crate::sketch::SketchDraft::new(
            shell.data.clone(),
        ))),
        ToolWindow::Import => shell.request_window(Draft::Import(ImportDraft {
            expected: shell.data.clone(),
            path: "missing.step".into(),
            name: "Absent".into(),
            bought: false,
            source: String::new(),
            licence: String::new(),
            designation: String::new(),
        })),
    }
    settle(h);
    assert_eq!(h.state().window.open, Some(kind));
}

const WINDOWS: [ToolWindow; 9] = [
    ToolWindow::CreateProject,
    ToolWindow::MoveRotate,
    ToolWindow::MoveCopy,
    ToolWindow::Export,
    ToolWindow::Checks,
    ToolWindow::Parameters,
    ToolWindow::Feature,
    ToolWindow::Sketch,
    ToolWindow::Import,
];

#[test]
fn every_window_closes_with_escape() {
    let (_dir, mut h) = fixture(1440., 1000.);
    for kind in WINDOWS {
        open(&mut h, kind);
        h.state_mut().window.error = Some("Test error".into());
        h.key_press(egui::Key::Escape);
        settle(&mut h);
        assert_eq!(h.state().window.open, None, "{kind:?}");
        assert!(h.query_by_label("Test error").is_none());
    }
}

#[test]
fn every_window_has_a_close_button() {
    let (_dir, mut h) = fixture(1440., 1000.);
    for kind in WINDOWS {
        open(&mut h, kind);
        h.get_by_label("Close window").click();
        settle(&mut h);
        assert_eq!(h.state().window.open, None, "{kind:?}");
    }
}

#[test]
fn move_rotate_window_closes() {
    let (_dir, mut h) = fixture(1024., 768.);
    for action in ["Escape", "Cancel placement", "Close window"] {
        open(&mut h, ToolWindow::MoveRotate);
        if action == "Escape" {
            h.key_press(egui::Key::Escape);
        } else {
            h.get_by_label(action).click();
        }
        settle(&mut h);
        assert!(h.state().placement_draft.is_none(), "{action}");
    }
}

#[test]
fn cancel_clears_the_window_error() {
    let (_dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::Import);
    h.get_by_label("Import file").click();
    h.step();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().saving && Instant::now() < deadline {
        h.step();
    }
    let error = h.state().window.error.clone().unwrap();
    h.get_by_label("Cancel import").click();
    settle(&mut h);
    open(&mut h, ToolWindow::Parameters);
    assert!(h.state().window.error.is_none());
    assert!(h.query_by_label(&error).is_none());
}

fn scroll_to_buttons(h: &mut Harness, title: &str, bounds: egui::Rect, buttons: &[&str]) {
    let layer = h
        .ctx
        .layer_id_at(h.get_by_label("Close window").rect().center())
        .expect(title);
    let window = h.ctx.memory(|m| m.area_rect(layer.id)).expect(title);
    assert!(
        bounds.expand(1.).contains_rect(window),
        "{window:?} outside {bounds:?}"
    );
    h.event(egui::Event::PointerMoved(window.center()));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., -1600.),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        h.step();
    }
    for label in buttons {
        let rect = h.get_by_label(label).rect();
        assert!(
            window.expand(1.).contains_rect(rect),
            "{label}: {rect:?} outside {window:?}"
        );
    }
}

#[test]
fn sketch_window_fits_a_768_pixel_screen() {
    let (_dir, mut h) = fixture(1024., 768.);
    open(&mut h, ToolWindow::Sketch);
    let bounds = h.ctx.content_rect();
    scroll_to_buttons(
        &mut h,
        "Create sketch",
        bounds,
        &["Create solid", "Cancel sketch"],
    );
}

#[test]
fn move_panel_buttons_stay_in_view() {
    let (_dir, mut h) = fixture(1024., 600.);
    open(&mut h, ToolWindow::MoveCopy);
    let bounds = h.get_by_label("Model viewport").rect();
    scroll_to_buttons(&mut h, "MOVE/COPY", bounds, &["OK", "Cancel"]);
}

#[test]
fn switching_feature_asks_before_losing_values() {
    let (_dir, mut h) = fixture(1440., 1000.);
    h.get_by_label("first").click();
    settle(&mut h);
    let label = crate::feature_form::label(&h.state().feature_draft.as_ref().unwrap().fields[0].0);
    h.get_by_label(&label).focus();
    h.step();
    h.get_by_label(&label).type_text("123");
    settle(&mut h);
    let typed = h.state().feature_draft.as_ref().unwrap().fields[0]
        .1
        .clone();
    assert!(typed.contains("123"));
    h.get_by_label("second").click();
    settle(&mut h);
    h.get_by_label("Keep").click();
    settle(&mut h);
    let form = h.state().feature_draft.as_ref().unwrap();
    assert_eq!(form.id, "first");
    assert_eq!(form.fields[0].1, typed);
}

#[test]
fn escape_in_a_pick_step_stops_only_the_pick() {
    let (_dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::MoveCopy);
    h.get_by_label("Point").click();
    settle(&mut h);
    h.get_all_by_label("Pick").last().unwrap().click();
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().move_draft.is_some());
    assert!(!h.state_mut().stop_pick());
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().move_draft.is_none());
}

#[test]
fn reload_preserves_the_dirty_form_and_disables_apply() {
    let (dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::Parameters);
    h.get_by_label("width").focus();
    h.step();
    h.get_by_label("width").type_text("123");
    settle(&mut h);
    let typed = h.state().parameter_draft.as_ref().unwrap().1["width"].clone();
    h.state_mut().request_window(Draft::Checks);
    assert!(h.state().window.next.is_some());
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    project
        .set_params([("width".into(), "35".into())].into())
        .unwrap();
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !h.state().window.stale && Instant::now() < deadline {
        h.step();
    }
    assert!(h.state().window.next.is_none());
    h.get_by_label("The project changed on disk. Apply is disabled until you reopen the form.");
    assert_eq!(
        h.state().parameter_draft.as_ref().unwrap().1["width"],
        typed
    );
    assert!(
        h.get_by_label("Apply parameters")
            .accesskit_node()
            .is_disabled()
    );
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().data.params["width"], "35");
    h.get_by_label("Cancel").click();
    settle(&mut h);
    open(&mut h, ToolWindow::Parameters);
    assert!(
        !h.get_by_label("Apply parameters")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn changed_controls_and_sketch_points_mark_the_form_dirty() {
    let (_dir, mut h) = fixture(1440., 1000.);
    for (kind, label) in [
        (ToolWindow::Import, "Purchased part"),
        (ToolWindow::Export, "STL meshes"),
        (ToolWindow::MoveCopy, "Create copy"),
        (ToolWindow::Sketch, "Sketch canvas"),
    ] {
        open(&mut h, kind);
        h.get_by_label(label).click();
        settle(&mut h);
        assert!(h.state().window.dirty, "{label}");
        h.state_mut().cancel_window();
    }
}

#[test]
fn reload_closes_a_clean_form() {
    let (dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::Parameters);
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    project
        .set_params([("width".into(), "47".into())].into())
        .unwrap();
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.params["width"] != "47" && Instant::now() < deadline {
        h.step();
    }
    assert!(h.state().parameter_draft.is_none());
}

fn pending_export(h: &mut Harness) -> mpsc::Sender<Exported> {
    open(h, ToolWindow::Export);
    h.get_by_label("Write files").click();
    h.step();
    assert!(h.state().export_pending.is_some());
    let (tx, rx) = mpsc::channel();
    h.state_mut().export_pending = Some(rx);
    tx
}

#[test]
fn failed_export_reports_only_to_the_window_that_started_it() {
    let (_dir, mut h) = fixture(1440., 1000.);
    let tx = pending_export(&mut h);
    h.state_mut().cancel_window();
    open(&mut h, ToolWindow::Export);
    tx.send(Err("Export test failure".into())).unwrap();
    settle(&mut h);
    assert_eq!(
        h.state().export_message.as_deref(),
        Some("Export test failure")
    );
    assert!(h.state().window.error.is_none());
}

#[test]
fn export_design_writes_the_assembly_as_one_3mf_file() {
    let (_dir, mut h) = fixture(1440., 1000.);
    let out = tempfile::tempdir().unwrap();
    open(&mut h, ToolWindow::Export);
    h.state_mut().export_path = out.path().display().to_string();
    h.get_by_label("3MF assembly").click();
    h.step();
    h.get_by_label("Write files").click();
    h.step();
    let start = std::time::Instant::now();
    while h.state().export_pending.is_some() {
        assert!(start.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(8));
        h.step();
    }
    let files = std::fs::read_dir(out.path())
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(files.len(), 1, "{:?}", h.state().export_message);
    let path = files[0].path();
    assert_eq!(path.extension().unwrap(), "3mf");
    assert!(std::fs::read(path).unwrap().starts_with(b"PK"));
}

#[test]
fn export_result_survives_a_project_switch() {
    let (_dir, mut h) = fixture(1440., 1000.);
    let tx = pending_export(&mut h);
    let other = tempfile::tempdir().unwrap();
    let project = faber_wiring::open(other.path(), true, Some("Other".into())).unwrap();
    let (send, receive) = mpsc::channel();
    send.send(Ok((project.data.clone(), project.bodies.clone())))
        .unwrap();
    h.state_mut().pending = Some(receive);
    h.state_mut().pending_project = Some(other.path().to_owned());
    settle(&mut h);
    assert_eq!(h.state().data.name, "Other");
    tx.send(Ok((3, Vec::new()))).unwrap();
    settle(&mut h);
    assert_eq!(
        h.state().export_message.as_deref(),
        Some("Exported 0 files from revision 3.")
    );
}

#[test]
fn checks_error_stays_with_the_form_that_started_the_check() {
    let (_dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::Checks);
    h.get_by_label("Run interference check").click();
    h.step();
    assert!(h.state().check_pending.is_some());
    let (tx, rx) = mpsc::channel();
    h.state_mut().check_pending = Some(rx);
    h.state_mut().cancel_window();
    open(&mut h, ToolWindow::Checks);
    tx.send((h.state().data.clone(), Err("Check failure".into())))
        .unwrap();
    settle(&mut h);
    assert_eq!(h.state().error.as_deref(), Some("Check failure"));
    assert!(h.state().window.error.is_none());
}

#[test]
fn move_fields_disable_while_the_move_saves() {
    let (_dir, mut h) = fixture(1440., 1000.);
    open(&mut h, ToolWindow::MoveCopy);
    h.key_press(egui::Key::Enter);
    h.step();
    assert!(h.state().saving);
    let (_tx, rx) = mpsc::channel();
    h.state_mut().pending = Some(rx);
    settle(&mut h);
    assert!(h.get_by_label("X distance").accesskit_node().is_disabled());
}
