use super::*;
use egui_kittest::{Harness, kittest::Queryable};

fn wait(h: &mut Harness<'_, Shell>, predicate: impl Fn(&Harness<'_, Shell>) -> bool) {
    let start = Instant::now();
    while !predicate(h) {
        h.step();
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "View operation timed out: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn views(h: &mut Harness<'_, Shell>) {
    for _ in 0..3 {
        h.step();
    }
    h.get_by_role_and_label(egui::Role::Button, "Views").click();
    wait(h, |h| h.query_by_label("Back to model").is_some());
}
fn enter(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(egui::Role::TextInput, label)
        .scroll_to_me();
    for _ in 0..4 {
        h.step();
    }
    h.get_by_role_and_label(egui::Role::TextInput, label)
        .click();
    h.step();
    h.get_by_role_and_label(egui::Role::TextInput, label)
        .type_text(text);
    h.step();
}
fn animation(h: &mut Harness<'_, Shell>) {
    h.get_by_label("Export animation").click();
    wait(h, |h| h.query_by_label("Write animation").is_some());
    h.get_by_role_and_label(egui::Role::SpinButton, "Frames")
        .click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("2".into()));
    h.step();
    h.key_press(egui::Key::Enter);
    h.step();
}

#[test]
fn named_view_restores_hidden_parts_camera_and_unsaved_section() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    views(&mut h);
    h.state_mut().hidden.insert("right".to_string());
    h.state_mut().section_on = true;
    h.state_mut().plane = Section {
        normal: [1., 0., 0.],
        offset: -22.,
        reverse: true,
    };
    let camera = h.state().camera;
    enter(&mut h, "View name", "Ön görünüş");
    h.get_by_label("Save view").click();
    h.step();
    wait(&mut h, |h| {
        h.query_by_label("Ön görünüş").is_some() && h.state().pending.is_none()
    });
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("analysis.json")).unwrap()).unwrap();
    assert_eq!(file["views"]["Ön görünüş"]["hidden"][0], "right");
    let section = file["views"]["Ön görünüş"]["section"].as_str().unwrap();
    assert!(file["sections"].get(section).is_some());
    h.state_mut().section_on = false;
    h.state_mut().hidden.clear();
    h.state_mut().camera.set_view(faber_scene::View::Front);
    h.get_by_label("Apply").click();
    h.step();
    assert_eq!(h.state().hidden, BTreeSet::from(["right".into()]));
    assert!(h.state().camera.eye().distance(camera.eye()) < 0.001);
    assert!(h.state().section_on && h.state().plane.reverse);
    assert!((h.state().plane.offset + 22.).abs() < 1e-6);
}

#[test]
fn explode_slider_keeps_camera_model_and_section_index_and_studio_paints_the_viewport() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    views(&mut h);
    let before = std::fs::read(dir.path().join("placement.json")).unwrap();
    let camera = h.state().camera;
    let indices = h.state().cut_indices.clone();
    let slider = h
        .get_by_role_and_label(egui::Role::Slider, "Explode")
        .rect();
    let at = egui::pos2(slider.right() - 2., slider.center().y);
    super::controls_fixture::click(
        &mut h,
        at,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert!(!h.state().move_poses().is_empty());
    assert_eq!(camera, h.state().camera);
    h.get_by_label("Studio").click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Studio ready").is_some());
    let studio = h.render().unwrap();
    let rect = super::controls_fixture::canvas(&h);
    if let Some(dir) = std::env::var_os("FABER_VIEWS_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        studio.save(dir.join("studio-explode.png")).unwrap();
    }
    h.get_by_label("Technical").click();
    h.step();
    let technical = h.render().unwrap();
    let delta = studio
        .enumerate_pixels()
        .filter(|(x, y, p)| {
            rect.shrink(30.).contains(egui::pos2(*x as f32, *y as f32))
                && technical.get_pixel(*x, *y) != *p
        })
        .count();
    assert!(delta > 1000, "Only {delta} changed pixels.");
    let p = super::controls_fixture::centre(&h, "left");
    assert_ne!(
        studio.get_pixel(p.x as u32, p.y as u32),
        technical.get_pixel(p.x as u32, p.y as u32)
    );
    assert!(std::sync::Arc::ptr_eq(&indices, &h.state().cut_indices));
    assert_eq!(
        before,
        std::fs::read(dir.path().join("placement.json")).unwrap()
    );
    assert_eq!(camera, h.state().camera);
}

#[test]
fn export_animation_from_the_viewer_writes_two_real_frames() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    views(&mut h);
    animation(&mut h);
    let path = dir.path().join("turn.apng");
    enter(&mut h, "Animation file", path.to_str().unwrap());
    h.get_by_label("Write animation").click();
    h.step();
    wait(&mut h, |_h| path.is_file());
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.get(..8), Some(b"\x89PNG\r\n\x1a\n".as_slice()));
    let i = bytes.windows(4).position(|s| s == b"acTL").unwrap();
    assert_eq!(
        u32::from_be_bytes(bytes[i + 4..i + 8].try_into().unwrap()),
        2
    );
    assert!(bytes.windows(4).any(|s| s == b"fdAT"));
}
#[test]
fn reloading_materials_while_studio_is_pending_discards_the_obsolete_upload() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    views(&mut h);
    let (tx, rx) = mpsc::channel();
    h.state_mut().views.studio_gate = Some(rx);
    h.get_by_label("Studio").click();
    h.step();
    h.get_by_label("Technical").click();
    h.step();
    let p = dir.path().join("parts/right.json");
    let mut j: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    j["material"] = "steel-s235".into();
    std::fs::write(p, serde_json::to_vec(&j).unwrap()).unwrap();
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    h.get_by_label("Studio").click();
    h.step();
    wait(&mut h, |h| {
        h.state().data.parts["right"].material.as_deref() == Some("steel-s235")
    });
    assert!(h.query_by_label("Studio ready").is_none());
    tx.send(()).unwrap();
    wait(&mut h, |h| h.query_by_label("Studio ready").is_some());
    let frame = h.render().unwrap();
    let colour = |id| {
        let p = super::controls_fixture::centre(&h, id);
        frame.get_pixel(p.x as u32, p.y as u32)
    };
    let (a, b) = (colour("right"), colour("left"));
    let diff = (0..3)
        .map(|i| i32::from(a[i]).abs_diff(i32::from(b[i])))
        .sum::<u32>();
    assert!(
        diff > 25,
        "The changed material and the original part differ by only {diff}."
    );
}

fn explode(h: &mut Harness<'_, Shell>, fraction: f32) {
    let rail = h
        .get_by_role_and_label(egui::Role::Slider, "Explode")
        .rect();
    super::controls_fixture::click(
        h,
        egui::pos2(rail.left() + fraction * rail.width(), rail.center().y),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
}
#[test]
fn a_changed_design_during_export_keeps_the_existing_file_and_the_camera() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    h.set_size(egui::vec2(1040., 850.));
    views(&mut h);
    animation(&mut h);
    let camera = h.state().camera;
    let rect = super::controls_fixture::canvas(&h);
    let path = dir.path().join("existing.apng");
    std::fs::write(&path, b"previous animation").unwrap();
    enter(&mut h, "Animation file", path.to_str().unwrap());
    let (tx, rx) = mpsc::channel();
    h.state_mut().views.export_gate = Some(rx);
    h.get_by_label("Write animation").click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Writing animation…").is_some());
    let before = h.get_by_label("Write animation").rect();
    let part = dir.path().join("parts/right.json");
    let mut j: serde_json::Value = serde_json::from_slice(&std::fs::read(&part).unwrap()).unwrap();
    j["name"] = "Revised right block".into();
    std::fs::write(part, serde_json::to_vec(&j).unwrap()).unwrap();
    for _ in 0..4 {
        h.step();
        assert_eq!(camera, h.state().camera);
        assert_eq!(rect, super::controls_fixture::canvas(&h));
        assert_eq!(before, h.get_by_label("Write animation").rect());
    }
    let _ = tx.send(());
    wait(&mut h, |h| {
        h.query_by_label_contains("project changed").is_some()
    });
    assert_eq!(std::fs::read(path).unwrap(), b"previous animation");
}
#[test]
fn studio_section_handle_can_be_dragged_with_an_exploded_pose_at_narrow_width() {
    let (_dir, mut h) = super::controls_fixture::two_blocks();
    h.set_size(egui::vec2(1030., 850.));
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.state_mut().plane = Section {
        normal: [1., 0., 0.],
        offset: 36.,
        reverse: false,
    };
    h.state_mut().section_on = true;
    views(&mut h);
    h.get_by_label("Studio").click();
    h.step();
    wait(&mut h, |h| h.query_by_label("Studio ready").is_some());
    explode(&mut h, 0.5);
    for _ in 0..4 {
        h.step();
    }
    let cam = h.state().camera;
    let cap = h.state().cap_poses.get("right").copied().unwrap();
    assert_ne!(cap, glam::Mat4::IDENTITY);
    let rect = super::controls_fixture::canvas(&h);
    let before = h.state().plane;
    let at = h
        .ctx
        .read_response(egui::Id::unique("section_offset_handle"))
        .unwrap()
        .rect
        .center();
    h.hover_at(at);
    h.step();
    h.drag_at(at);
    h.step();
    let end = at + egui::vec2(19., -8.);
    h.hover_at(end);
    h.step();
    h.drop_at(end);
    h.step();
    assert_ne!(h.state().plane.offset, before.offset);
    assert_eq!(h.state().camera, cam);
    assert_eq!(h.state().cap_poses.get("right"), Some(&cap));
    assert_eq!(h.state().cap_plane, Some(h.state().plane));
    assert_eq!(rect, super::controls_fixture::canvas(&h));
    assert!(h.state().caps.is_some());
}
#[test]
fn picking_the_exploded_face_hits_the_visible_part_beyond_its_assembled_bounds() {
    let (_dir, mut h) = super::controls_fixture::two_blocks();
    views(&mut h);
    explode(&mut h, 0.98);
    h.state_mut().set_filter(SelectFilter::Face);
    h.step();
    let rect = super::controls_fixture::canvas(&h);
    let body = h.state().bodies.iter().find(|b| b.id == "right").unwrap();
    let dx = f64::from(h.state().move_poses()["right"].w_axis.x);
    assert!(dx > 1.);
    let x = body.bounds[1][0] + dx * 0.6;
    let target = glam::DVec3::new(x, 7.5, body.bounds[1][2]);
    let pos =
        super::move_tool::screen_point(rect, h.state().camera.matrix(rect.aspect_ratio()), target)
            .unwrap();
    super::controls_fixture::click(
        &mut h,
        pos,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert!(matches!(h.state().picks.last(),Some(Pick::Face{part,..}) if part=="right"));
    assert!((h.state().pick_point.unwrap()[0] - x).abs() < 1e-3);
}
#[test]
fn loaded_assembly_reuses_studio_geometry_and_indices_during_21_drag_steps() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion");
    let p = faber_wiring::read_snapshot(&root).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280., 850.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, p.data, p.bodies, None));
    views(&mut h);
    h.get_by_label("Studio").click();
    wait(&mut h, |h| h.query_by_label("Studio ready").is_some());
    let first = h.state().frame_ms;
    let uploads = h.state().views.uploads;
    let indices = h.state().cut_indices.clone();
    let mut slider = Vec::new();
    let rail = h
        .get_by_role_and_label(egui::Role::Slider, "Explode")
        .rect();
    let start = egui::pos2(rail.left() + 4., rail.center().y);
    h.hover_at(start);
    h.step();
    h.drag_at(start);
    h.step();
    let mut factor = h.state().views.explode;
    let mut slider_frames = Vec::new();
    for i in 1..=21 {
        h.hover_at(start + egui::vec2(i as f32 * 4., 0.));
        let started = Instant::now();
        h.step();
        slider_frames.push(started.elapsed().as_secs_f64() * 1000.);
        assert_ne!(factor, h.state().views.explode);
        factor = h.state().views.explode;
        slider.push(h.state().frame_ms);
    }
    h.drop_at(start + egui::vec2(84., 0.));
    h.step();
    h.state_mut().section_on = true;
    h.state_mut().plane = Section {
        normal: [1., 0., 0.],
        offset: 8.,
        reverse: false,
    };
    for _ in 0..4 {
        h.step();
    }
    let control = h
        .ctx
        .read_response(egui::Id::unique("section_offset_handle"))
        .unwrap()
        .rect
        .center();
    h.hover_at(control);
    h.step();
    h.drag_at(control);
    h.step();
    let mut section = Vec::new();
    let mut section_frames = Vec::new();
    let mut old = h.state().plane.offset;
    for i in 1..=21 {
        h.hover_at(control + egui::vec2(3. * i as f32, -(i as f32)));
        let started = Instant::now();
        h.step();
        section_frames.push(started.elapsed().as_secs_f64() * 1000.);
        if i > 1 {
            assert_ne!(old, h.state().plane.offset);
        }
        old = h.state().plane.offset;
        section.push(h.state().frame_ms);
    }
    h.drop_at(control + egui::vec2(63., -21.));
    h.step();
    assert_eq!(h.state().views.uploads, uploads);
    assert!(std::sync::Arc::ptr_eq(&indices, &h.state().cut_indices));
    for (name, mut values) in [
        ("explode submit", slider),
        ("section submit", section),
        ("explode input frame", slider_frames),
        ("section input frame", section_frames),
    ] {
        values.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: first studio {first:.2}, median {:.2}, p95 {:.2}, max {:.2} ms",
            values[10], values[19], values[20]
        );
    }
}
#[test]
fn the_viewport_shows_the_active_orthographic_projection() {
    let (_d, mut h) = super::controls_fixture::two_blocks();
    h.state_mut().camera.lens.ortho_height = Some(70.);
    h.state_mut().dirty = true;
    h.step();
    assert!(
        h.output().shapes.iter().any(
            |item| matches!(&item.shape,egui::Shape::Text(t) if t.galley.text()=="ORTHOGRAPHIC")
        ),
        "The projection label must match the named camera."
    );
}
