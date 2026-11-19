use super::controls_fixture::{canvas, click, two_blocks};
use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::Value;

#[track_caller]
fn wait(h: &mut Harness<'_, Shell>, f: impl Fn(&Harness<'_, Shell>) -> bool) {
    let until = Instant::now() + Duration::from_secs(25);
    while !f(h) {
        h.step();
        if Instant::now() >= until
            && let Some(dir) = std::env::var_os("FABER_FORM_CAPTURE")
        {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            h.render()
                .unwrap()
                .save(dir.join("unfinished.png"))
                .unwrap();
        }
        assert!(
            Instant::now() < until,
            "Form edit timed out: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn press(h: &mut Harness<'_, Shell>, name: &str) {
    h.get_by_role_and_label(egui::Role::Button, name)
        .scroll_to_me();
    // The feature list is inside the browser scroll area. Let both scrolls reach the row.
    let mut prev = None;
    let mut still = 0;
    for _ in 0..40 {
        h.step();
        let r = h.get_by_role_and_label(egui::Role::Button, name).rect();
        if prev == Some(r) {
            still += 1;
        } else {
            still = 0;
        }
        prev = Some(r);
        if still >= 3 {
            break;
        }
    }
    h.get_by_role_and_label(egui::Role::Button, name).click();
    h.step();
}
fn opened() -> (tempfile::TempDir, Harness<'static, Shell>) {
    let (d, mut h) = two_blocks();
    press(&mut h, "New form");
    wait(&mut h, |h| h.query_by_label("Create form").is_some());
    press(&mut h, "Create form");
    wait(&mut h, |h| {
        h.query_by_label("Level 2 preview").is_some() && h.state().pending.is_none()
    });
    (d, h)
}
fn cage(root: &std::path::Path) -> Value {
    let d: Value =
        serde_json::from_slice(&std::fs::read(root.join("parts/form_1.json")).unwrap()).unwrap();
    d["geometry"]["cage"].clone()
}
fn top(h: &Harness<'_, Shell>, x: f64, y: f64) -> egui::Pos2 {
    let r = canvas(h);
    super::move_tool::screen_point(
        r,
        h.state().camera.matrix(r.aspect_ratio()),
        glam::DVec3::new(x, y, 20.),
    )
    .unwrap()
}
#[test]
fn form_mode_shows_the_cage_and_finish_restores_the_final_mesh() {
    let (d, mut h) = opened();
    let n = h.state().data.revision;
    assert_eq!(cage(d.path())["vertices"].as_array().unwrap().len(), 8);
    let picture = h.render().unwrap();
    let (width, height) = picture.dimensions();
    let mut light: Vec<_> = picture
        .enumerate_pixels()
        .filter_map(|(x, y, p)| {
            let [r, g, b, _] = p.0;
            (x > width * 2 / 5
                && x < width * 3 / 5
                && y > height / 3
                && y < height * 2 / 3
                && r > 50
                && i32::from(g) - i32::from(r) > 35
                && i32::from(b) - i32::from(r) > 35)
                .then_some((u32::from(r) * 21 + u32::from(g) * 72 + u32::from(b) * 7) / 100)
        })
        .collect();
    light.sort_unstable();
    assert!(light.len() > 1000);
    assert!(
        light[light.len() * 9 / 10] - light[light.len() / 10] >= 12,
        "The smooth form must show a range of lit surface tones."
    );
    if let Some(dir) = std::env::var_os("FABER_FORM_CAPTURE") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        picture.save(dir.join("form-cage.png")).unwrap();
    }
    let r = canvas(&h);
    press(&mut h, "Finish form");
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_none());
    assert_eq!(h.state().data.revision, n);
    let other = h.render().unwrap();
    assert!(
        picture
            .enumerate_pixels()
            .filter(
                |(x, y, p)| r.shrink(20.).contains(egui::pos2(*x as f32, *y as f32))
                    && other.get_pixel(*x, *y) != *p
            )
            .count()
            > 300
    );
    assert_eq!(
        h.state()
            .bodies
            .iter()
            .find(|b| b.id == "form_1")
            .unwrap()
            .mesh
            .triangles
            .len(),
        768
    );
    press(&mut h, "Edit form");
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_some());
    let row = |h: &Harness<'_, Shell>| h.get_by_role_and_label(egui::Role::Button, "right").rect();
    let before = row(&h);
    for _ in 0..6 {
        h.step();
        assert!(
            (row(&h).center() - before.center()).length() < 1.,
            "The browser rows moved while the form was open."
        );
    }
    h.get_by_role_and_label(egui::Role::Button, "right").click();
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_none());
    assert_eq!(h.state().selected.as_deref(), Some("right"));
    assert_eq!(h.state().data.revision, n);
}
#[test]
fn face_drag_moves_four_vertices_in_one_revision() {
    let (d, mut h) = opened();
    let before = cage(d.path());
    let revision = h.state().data.revision;
    let camera = h.state().camera;
    h.key_press(egui::Key::Num3);
    h.step();
    let pos = top(&h, 0., 0.);
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("1 face selected");
    h.drag_at(pos);
    h.step();
    let mut frames = Vec::new();
    for i in 1..=12 {
        h.hover_at(pos + egui::vec2(i as f32 * 2., 0.));
        let started = Instant::now();
        h.step();
        frames.push(started.elapsed().as_secs_f64() * 1000.);
        assert_eq!(h.state().data.revision, revision);
        assert_eq!(h.state().camera, camera);
    }
    frames.sort_by(f64::total_cmp);
    eprintln!(
        "Form mouse input frame: median {:.2} ms, max {:.2} ms",
        frames[6], frames[11]
    );
    h.drop_at(pos + egui::vec2(24., 0.));
    h.step();
    wait(&mut h, |h| h.state().data.revision == revision + 1);
    let after = cage(d.path());
    assert_eq!(
        before["vertices"]
            .as_array()
            .unwrap()
            .iter()
            .zip(after["vertices"].as_array().unwrap())
            .filter(|(a, b)| a != b)
            .count(),
        4
    );
}
#[test]
fn extrude_button_adds_faces_and_symmetry_switch_mirrors_an_edit() {
    let (d, mut h) = opened();
    let revision = h.state().data.revision;
    h.get_by_label("Symmetry X").click();
    h.step();
    wait(&mut h, |h| {
        h.state().data.revision == revision + 1 && h.query_by_label("Level 2 preview").is_some()
    });
    h.key_press(egui::Key::Num3);
    h.step();
    let pos = top(&h, 0., 0.);
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("1 face selected");
    press(&mut h, "Extrude");
    wait(&mut h, |h| h.state().data.revision == revision + 2);
    let data = cage(d.path());
    assert_eq!(data["faces"].as_array().unwrap().len(), 10);
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_some());
    h.key_press(egui::Key::Num1);
    h.step();
    let vs = data["vertices"].as_array().unwrap();
    let index = |x: f64| {
        vs.iter()
            .enumerate()
            .filter(|(_, p)| p[0].as_f64() == Some(x) && p[1].as_f64() == Some(10.))
            .max_by(|a, b| {
                a.1[2]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b.1[2].as_f64().unwrap())
            })
            .unwrap()
            .0
    };
    let (a, b) = (index(10.), index(-10.));
    let vertex = |c: &Value, i: usize| {
        glam::DVec3::from_array(std::array::from_fn(|j| {
            c["vertices"][i][j].as_f64().unwrap()
        }))
    };
    let start = vertex(&data, a);
    let other = vertex(&data, b);
    let rect = canvas(&h);
    let at =
        super::move_tool::screen_point(rect, h.state().camera.matrix(rect.aspect_ratio()), start)
            .unwrap();
    click(
        &mut h,
        at,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("1 vertex selected");
    drag(&mut h, at, egui::vec2(12., -6.));
    wait(&mut h, |h| h.state().data.revision == revision + 3);
    let c = cage(d.path());
    let d1 = vertex(&c, a) - start;
    let d2 = vertex(&c, b) - other;
    assert!(d1.length() > 0.1 && d2.distance(d1 * glam::dvec3(-1., 1., 1.)) < 1e-4);
}
fn drag(h: &mut Harness<'_, Shell>, pos: egui::Pos2, delta: Vec2) {
    h.hover_at(pos);
    h.step();
    h.drag_at(pos);
    h.step();
    for i in 1..=5 {
        h.hover_at(pos + delta * (i as f32 / 5.));
        h.step();
    }
    h.drop_at(pos + delta);
    h.step();
}
#[test]
fn rotate_and_scale_a_picked_face_keep_the_centre_and_survive_undo() {
    let (d, mut h) = opened();
    let first = cage(d.path());
    let rev = h.state().data.revision;
    h.key_press(egui::Key::Num3);
    h.step();
    let pos = top(&h, 0., 0.);
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    fn radii(c: &Value) -> Vec<f64> {
        c["vertices"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| {
                let (z, x, y) = (v[2].as_f64()?, v[0].as_f64()?, v[1].as_f64()?);
                (z > 19.99).then_some(x.hypot(y))
            })
            .collect()
    }
    h.get_by_label("Rotate").click();
    h.step();
    drag(&mut h, pos, egui::vec2(30., -10.));
    wait(&mut h, |h| {
        h.state().data.revision == rev + 1 && h.query_by_label("Level 2 preview").is_some()
    });
    let turn = cage(d.path());
    assert_ne!(turn, first);
    let saved = faber_wiring::read_snapshot(d.path()).unwrap();
    assert_eq!(
        saved.data.parts["form_1"].geometry,
        h.state().data.parts["form_1"].geometry,
        "The next edit must read the same vertex coordinates the rotation saved."
    );
    for (a, b) in radii(&first).iter().zip(radii(&turn)) {
        assert!((a - b).abs() < 1e-4);
    }
    h.get_by_label("Scale selection").click();
    h.step();
    drag(&mut h, pos, egui::vec2(14., -5.));
    wait(&mut h, |h| h.state().data.revision == rev + 2);
    let scale = cage(d.path());
    assert!(radii(&scale).iter().zip(radii(&turn)).all(|(a, b)| a > &b));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.step();
    wait(&mut h, |h| {
        h.state().data.revision > rev + 2 && cage(d.path()) == turn
    });
}
#[test]
fn picking_an_edge_adds_a_crease_then_a_loop() {
    let (d, mut h) = opened();
    let rev = h.state().data.revision;
    h.key_press(egui::Key::Num2);
    h.step();
    let pos = top(&h, 0., 10.);
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("1 edge selected");
    press(&mut h, "Crease");
    wait(&mut h, |h| {
        h.state().data.revision == rev + 1 && h.query_by_label("Level 2 preview").is_some()
    });
    assert_eq!(cage(d.path())["creases"][0]["sharpness"], 2.);
    press(&mut h, "Insert loop");
    wait(&mut h, |h| h.state().data.revision == rev + 2);
    assert_eq!(cage(d.path())["vertices"].as_array().unwrap().len(), 12);
}
#[test]
fn the_editable_handle_has_a_solid_socket_and_a_level_two_grip_preview() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-handle");
    let p = faber_wiring::read_snapshot(&root).unwrap();
    let body = &p.bodies[0];
    assert!(body.shape.is_some() && body.volume > 1000.);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1320., 850.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, p.data, p.bodies, None));
    for _ in 0..4 {
        h.step();
    }
    let pos = super::controls_fixture::centre(&h, "handle");
    click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    press(&mut h, "Edit form");
    for _ in 0..3 {
        h.step();
    }
    if let Some(dir) = std::env::var_os("FABER_FORM_CAPTURE") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render()
            .unwrap()
            .save(dir.join("handle-open.png"))
            .unwrap();
    }
    assert!(
        h.state().form.is_some(),
        "The grip feature did not open; selected={:?}, error={:?}",
        h.state().selected,
        h.state().error
    );
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_some());
    assert!(h.query_by_label("Symmetry X").is_some());
    if let Some(dir) = std::env::var_os("FABER_FORM_CAPTURE") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render()
            .unwrap()
            .save(dir.join("handle-cage.png"))
            .unwrap();
    }
    press(&mut h, "Finish form");
    h.step();
    if let Some(dir) = std::env::var_os("FABER_FORM_CAPTURE") {
        h.render()
            .unwrap()
            .save(PathBuf::from(dir).join("handle-solid.png"))
            .unwrap();
    }
    // A short browser scrolls independently of the feature list inside it.
    let visible = canvas(&h).y_range().shrink(20.);
    for _ in 0..15 {
        let centre = h
            .get_by_role_and_label(egui::Role::Button, "grip")
            .rect()
            .center();
        if visible.contains(centre.y) {
            break;
        }
        h.hover_at(egui::pos2(128., visible.center()));
        h.step();
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., if centre.y > visible.max { -100. } else { 100. }),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        });
        for _ in 0..12 {
            h.step();
        }
    }
    let grip = h.get_by_role_and_label(egui::Role::Button, "grip");
    assert!(
        visible.contains(grip.rect().center().y),
        "The feature row must be in the browser before clicking."
    );
    grip.click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Level 2 preview").is_some());
}
#[test]
fn delete_in_vertex_mode_keeps_the_part_and_the_revision() {
    let (d, mut h) = opened();
    let rev = h.state().data.revision;
    let at = top(&h, 10., 10.);
    click(
        &mut h,
        at,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.get_by_label("1 vertex selected");
    h.key_press(egui::Key::Delete);
    for _ in 0..4 {
        h.step();
    }
    assert!(h.state().pending.is_none());
    assert!(d.path().join("parts/form_1.json").exists());
    assert_eq!(h.state().data.revision, rev);
}
#[test]
fn an_external_cage_change_during_drag_discards_the_old_preview_and_preserves_the_edit() {
    let (d, mut h) = opened();
    let rev = h.state().data.revision;
    h.key_press(egui::Key::Num3);
    h.step();
    let start = top(&h, 0., 0.);
    click(
        &mut h,
        start,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    h.drag_at(start);
    h.step();
    h.hover_at(start + egui::vec2(16., 0.));
    h.step();
    let file = d.path().join("parts/form_1.json");
    let mut changed: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    for vertex in changed["geometry"]["cage"]["vertices"]
        .as_array_mut()
        .unwrap()
    {
        vertex[0] = Value::from(vertex[0].as_f64().unwrap() * 0.75);
    }
    std::fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| {
        h.state().pending.is_none()
            && h.state()
                .bodies
                .iter()
                .find(|b| b.id == "form_1")
                .is_some_and(|b| b.bounds[1][0] < 7.5)
    });
    h.drop_at(start + egui::vec2(16., 0.));
    h.step();
    wait(&mut h, |h| {
        h.query_by_label("Level 2 preview").is_some() && h.state().pending.is_none()
    });
    assert_eq!(cage(d.path()), changed["geometry"]["cage"]);
    assert_eq!(h.state().data.revision, rev);
}
