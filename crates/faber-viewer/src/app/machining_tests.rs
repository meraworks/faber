//! Exercise the machining panel with the native renderer and real pointer input.
use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

fn wait(h: &mut Harness<'_, Shell>, condition: impl Fn(&Harness<'_, Shell>) -> bool) {
    let started = Instant::now();
    while !condition(h) {
        h.step();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "The machining result did not arrive: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}
fn setup_file(depth: f64) -> serde_json::Value {
    json!({"schema":1,"machining":{"tools":{"mill":{"kind":"flat","diameter":4,"flutes":2,"flute_length":18,"feed_mm_min":400,"plunge_mm_min":100,"spindle_rpm":6000,"number":1}},"setups":{"top":{"part":"right","origin":"stock_top_corner","up":[0,0,1],"stock":{"kind":"offset","sides":2,"bottom":0,"top":1}}},"operations":[{"id":"face_top","setup":"top","tool":"mill","heights":{"top":0,"bottom":depth,"retract":2,"clearance":5},"kind":"face","stepover":2}]}})
}
fn fixture(depth: f64) -> (tempfile::TempDir, Harness<'static, Shell>) {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    h.get_by_role_and_label(egui::Role::Button, "Manufacture")
        .click();
    h.step();
    let file = setup_file(depth);
    std::fs::write(
        dir.path().join("manufacture.json"),
        serde_json::to_vec(&file).unwrap(),
    )
    .unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| h.query_by_label("Toolpaths ready").is_some());
    (dir, h)
}
#[test]
fn operation_list_shows_times() {
    let (_dir, mut h) = fixture(-1.);
    h.get_by_role_and_label(egui::Role::Button, "face_top")
        .click();
    h.step();
    h.get_by_label("mill");
    assert!(
        h.query_all_by_label_contains("Operation time ")
            .next()
            .is_some(),
        "Missing operation time."
    );
}
#[test]
fn simulate_shows_the_cut_stock() {
    let (_d, mut h) = fixture(-1.);
    let before = h.render().unwrap();
    h.get_by_label("Simulate").click();
    h.step();
    let after = h.render().unwrap();
    let r = super::controls_fixture::canvas(&h).shrink(50.);
    let changed = before
        .enumerate_pixels()
        .filter(|(x, y, c)| {
            r.contains(egui::pos2(*x as f32, *y as f32)) && *c != after.get_pixel(*x, *y)
        })
        .count();
    assert!(changed > 500, "Only {changed} changed stock pixels.");
    h.get_by_label("Cut stock");
}
#[test]
fn gouge_is_marked_red() {
    let (_d, mut h) = fixture(-2.);
    h.get_by_label("Simulate").click();
    h.step();
    h.get_by_label("Gouge 1.00 mm");
    let r = super::controls_fixture::canvas(&h).shrink(30.);
    let pic = h.render().unwrap();
    let red = pic
        .enumerate_pixels()
        .filter(|(x, y, c)| {
            r.contains(egui::pos2(*x as f32, *y as f32)) && c[0] > 180 && c[1] < 100 && c[2] < 95
        })
        .count();
    assert!(red > 500, "Only {red} gouge pixels.");
    if let Some(dir) = std::env::var_os("FABER_CAM_CAPTURE") {
        let p = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&p).unwrap();
        pic.save(p.join("gouge.png")).unwrap();
    }
}

#[test]
fn machining_refresh_preserves_camera_and_paths_while_the_worker_checks_a_new_setup_revision() {
    use egui_kittest::kittest::NodeT;
    let (dir, mut h) = fixture(-1.);
    h.set_size(egui::vec2(1024., 780.));
    for _ in 0..5 {
        h.step();
    }
    let viewport = super::controls_fixture::canvas(&h);
    let source = h.state().camera;
    let start = viewport.center();
    h.event(egui::Event::PointerMoved(start));
    h.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    h.step();
    let end = start + egui::vec2(64., 18.);
    h.event(egui::Event::PointerMoved(end));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.step();
    assert_ne!(h.state().camera.yaw, source.yaw);
    let camera = h.state().camera;
    let row = h.get_by_label("Simulate").rect();
    let initial = h.render().unwrap();
    let (tx, rx) = mpsc::channel();
    h.state_mut().machining.as_mut().unwrap().gate = Some(rx);
    let file = dir.path().join("manufacture.json");
    let mut f: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    f["machining"]["operations"][0]["heights"]["bottom"] = json!(-2.);
    std::fs::write(&file, serde_json::to_vec(&f).unwrap()).unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| {
        h.query_by_label("Updating the previous toolpaths…")
            .is_some()
    });
    for _ in 0..5 {
        h.step();
        let sim = h.get_by_label("Simulate");
        assert!(sim.accesskit_node().is_disabled());
        assert_eq!(sim.rect(), row);
        assert_eq!(camera, h.state().camera);
        assert_eq!(super::controls_fixture::canvas(&h), viewport);
    }
    let second = h.render().unwrap();
    let area = viewport.shrink(42.);
    for (x, y, c) in initial
        .enumerate_pixels()
        .filter(|(x, y, _)| area.contains(egui::pos2(*x as f32, *y as f32)))
    {
        assert_eq!(c, second.get_pixel(x, y));
    }
    tx.send(()).unwrap();
    wait(&mut h, |h| h.query_by_label("Toolpaths ready").is_some());
    assert_eq!(h.state().camera, camera);
    h.get_by_label("Simulate").click();
    h.step();
    h.get_by_label("Gouge 1.00 mm");
}

#[test]
fn machining_shows_moves_in_the_placed_setup_and_restores_the_model_camera() {
    let (_dir, mut h) = fixture(-1.);
    // The second body starts at world X=30. Its machine origin is on its stock corner.
    assert!(h.state().camera.target.x > 30. && h.state().camera.target.x < 45.);
    let pic = h.render().unwrap();
    let area = super::controls_fixture::canvas(&h).shrink(10.);
    let count = |test: fn([u8; 4]) -> bool| {
        pic.enumerate_pixels()
            .filter(|(x, y, p)| area.contains(egui::pos2(*x as f32, *y as f32)) && test(p.0))
            .count()
    };
    assert!(
        count(|p| p[2] > 160 && p[0] < 105 && p[1] < 180) > 100,
        "Missing blue feed moves."
    );
    assert!(
        count(|p| p[0] > 180 && p[1] > 135 && p[2] < 115) > 10,
        "Missing yellow rapid moves."
    );
    if let Some(dir) = std::env::var_os("FABER_CAM_CAPTURE") {
        let p = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&p).unwrap();
        pic.save(p.join("setup.png")).unwrap();
    }
    let camera = h.state().camera;
    let start = area.center();
    h.hover_at(start);
    h.step();
    let middle = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(middle(start, true));
    h.step();
    let end = start + egui::vec2(70., -30.);
    h.event(egui::Event::PointerMoved(end));
    h.step();
    h.event(middle(end, false));
    h.step();
    assert!(camera.target.distance(h.state().camera.target) > 0.5);
    assert_eq!(camera.distance, h.state().camera.distance);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., 100.),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    });
    for _ in 0..5 {
        h.step();
    }
    assert!(h.state().camera.distance < camera.distance * 0.99);
    h.get_by_label("Back to model").click();
    h.step();
    assert!(h.state().camera.target.x < 15.);
}

#[test]
fn generated_instance_uses_its_setup_in_the_component_frame() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    std::fs::write(root.join("design.jsonnet"),r#"local f=import 'faber.libsonnet'; {
        components:{station:f.component('Station',translation=[50,40,20],rotation=[0,90,0])},
        parts:{source:f.part('Blank',[f.box('block',[15,15,15])],'block'), copy:f.instance('Second blank','source',component='station')}}
    "#).unwrap();
    faber_wiring::design_source().build(&root).unwrap();
    let mut file = setup_file(-1.);
    file["machining"]["setups"]["top"]["part"] = json!("source");
    let mut station = file["machining"]["setups"]["top"].clone();
    station["part"] = json!("copy");
    file["machining"]["setups"]["station"] = station;
    let mut other = file["machining"]["operations"][0].clone();
    other["id"] = json!("face_station");
    other["setup"] = json!("station");
    file["machining"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(other);
    std::fs::write(
        root.join("manufacture.json"),
        serde_json::to_vec(&file).unwrap(),
    )
    .unwrap();
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(980., 780.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    h.state_mut().select_only(Some("source".into()));
    h.step();
    h.get_by_label("Manufacture").click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Toolpaths ready").is_some());
    h.get_by_label("Part source");
    h.get_by_role_and_label(egui::Role::ComboBox, "Setup")
        .click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("station").click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Toolpaths ready").is_some());
    h.get_by_label("Part copy");
    h.get_by_label("face_station");
    let centre = h.state().camera.target;
    assert!(
        (55.0..64.).contains(&centre.x)
            && (43.0..51.).contains(&centre.y)
            && (8.0..17.).contains(&centre.z),
        "Setup centre {centre:?}"
    );
}
