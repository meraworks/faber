use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Harness<'static, Shell>) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Surface study".into())).unwrap();
    let profile = |z: f64| json!({"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[20,20]}},"placement":{"translation":[0,0,z],"rotation":[0,0,0]}});
    let part:faber_model::Part<faber_model::Scalar>=serde_json::from_value(json!({
        "name":"Housing","colour":[150,145,140],"geometry":{"op":"history","output":"block","features":[
            {"id":"block","geometry":{"op":"box","size":[20,20,10]}},{"id":"rim","geometry":profile(15.)},
            {"id":"top","geometry":profile(30.)},{"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[0,0,1]}},
            {"id":"guide","geometry":{"op":"curve","path":{"start":[0,0,15],"segments":[{"kind":"line","end":[0,0,30]}]}}}
        ]}})).unwrap();
    p.put_part("housing".into(), part).unwrap();
    let root = dir.path().to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 900.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, p.data, p.bodies, None));
    h.state_mut().select_only(Some("housing".into()));
    h.step();
    (dir, h)
}
fn button(h: &mut Harness<'_, Shell>, s: &str) {
    h.get_by_role_and_label(egui::Role::Button, s)
        .scroll_to_me();
    for _ in 0..4 {
        h.step();
    }
    let b = h.get_by_role_and_label(egui::Role::Button, s);
    assert!(!b.accesskit_node().is_disabled(), "{s} is disabled");
    b.click();
    for _ in 0..3 {
        h.step();
    }
}
fn wait(h: &mut Harness<'_, Shell>, test: impl Fn(&Harness<'_, Shell>) -> bool) {
    let start = Instant::now();
    while !test(h) {
        h.step();
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "Unfinished surface task: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}
fn patch(h: &mut Harness<'_, Shell>) {
    let rev = h.state().data.revision;
    button(h, "Surface tools");
    button(h, "Patch");
    button(h, "Apply feature");
    wait(h, |h| h.state().data.revision == rev + 1);
    button(h, "patch_1");
    wait(h, |h| h.query_by_label("4 open edges").is_some());
}
#[test]
fn surface_features_are_in_the_feature_list() {
    let (_d, mut h) = fixture();
    button(&mut h, "Surface tools");
    for label in [
        "Surface extrude",
        "Surface revolve",
        "Surface loft",
        "Surface sweep",
        "Patch",
        "Trim surface",
        "Extend surface",
        "Stitch",
        "Split with surface",
        "Replace face",
    ] {
        h.get_by_role_and_label(egui::Role::Button, label);
    }
    button(&mut h, "Surface extrude");
    h.get_by_role_and_label(egui::Role::ComboBox, "Profile");
    button(&mut h, "Apply feature");
    wait(&mut h, |h| {
        h.query_by_role_and_label(egui::Role::Button, "surface_extrude_1")
            .is_some()
    });
    let part = &h.state().data.parts["housing"];
    let g = serde_json::to_value(&part.geometry).unwrap();
    assert_eq!(g["output"], "block");
    assert_eq!(
        g["features"].as_array().unwrap().last().unwrap()["geometry"]["profile"],
        "rim"
    );
}
#[test]
fn open_edges_are_highlighted() {
    let (_d, mut h) = fixture();
    patch(&mut h);
    let rect = super::controls_fixture::canvas(&h);
    h.state_mut().camera.set_view(faber_scene::View::Top);
    h.state_mut().dirty = true;
    h.step();
    let img = h.render().unwrap();
    let orange = |c: [u8; 4]| c[0] > 200 && c[1] > 65 && c[1] < 175 && c[2] < 85;
    let pixels = img
        .enumerate_pixels()
        .filter(|(x, y, c)| rect.contains(egui::pos2(*x as f32, *y as f32)) && orange(c.0))
        .count();
    assert!(pixels > 250, "Only {pixels} orange boundary pixels.");
    if let Some(dir) = std::env::var_os("FABER_SURFACE_FEATURE_CAPTURE") {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p).unwrap();
        img.save(p.join("patch-top.png")).unwrap();
    }
}
#[test]
fn surface_feature_is_visible_when_selected() {
    let (_d, mut h) = fixture();
    patch(&mut h);
    for view in [faber_scene::View::Top, faber_scene::View::Bottom] {
        h.state_mut().camera.set_view(view);
        h.state_mut().dirty = true;
        h.step();
        let vp = super::controls_fixture::canvas(&h);
        let centre = move_tool::screen_point(
            vp,
            h.state().camera.matrix(vp.aspect_ratio()),
            glam::dvec3(10., 10., 15.),
        )
        .unwrap();
        let c = h
            .render()
            .unwrap()
            .get_pixel(centre.x as u32, centre.y as u32)
            .0;
        assert!(
            c[2] > c[0] + 15 && c[1] > c[0] + 10,
            "Missing surface on {view:?}: {c:?}"
        );
    }
    let camera = h.state().camera.matrix(1.);
    button(&mut h, "Show solid");
    assert!(h.query_by_label("4 open edges").is_none());
    assert_eq!(camera, h.state().camera.matrix(1.));
}

#[test]
fn all_surface_recipes_have_typed_inputs_and_a_placed_edge_pick_extends_the_patch() {
    let (dir, mut h) = fixture();
    patch(&mut h);
    button(&mut h, "Show solid");
    for (label, field) in [
        ("Surface extrude", "Profile"),
        ("Surface revolve", "Reference axis"),
        ("Surface loft", "Section 1"),
        ("Surface sweep", "Sweep path"),
        ("Patch", "Boundary 1"),
        ("Trim surface", "Cutting tool"),
        ("Stitch", "Surface 1"),
        ("Split with surface", "Cutting tool"),
        ("Replace face", "Surface"),
    ] {
        button(&mut h, "Surface tools");
        button(&mut h, label);
        h.get_by_role_and_label(egui::Role::ComboBox, field);
        button(&mut h, "Cancel feature");
    }
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut part = p.data.parts["housing"].clone();
    part.placement.translation = [40., 10., 5.].map(faber_model::Scalar::Number);
    part.placement.rotation = [0., 0., 90.].map(faber_model::Scalar::Number);
    p.put_part("housing".into(), part).unwrap();
    let rev = p.data.revision;
    drop(p);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| h.state().data.revision == rev);
    h.state_mut().fit();
    h.state_mut().camera.set_view(faber_scene::View::Top);
    h.step();
    button(&mut h, "Surface tools");
    button(&mut h, "Extend surface");
    button(&mut h, "Pick surface edge");
    wait(&mut h, |h| h.query_by_label("4 open edges").is_some());
    button(&mut h, "Cancel edge pick");
    button(&mut h, "Cancel feature");
    button(&mut h, "Surface tools");
    button(&mut h, "Extend surface");
    button(&mut h, "Pick surface edge");
    wait(&mut h, |h| h.query_by_label("4 open edges").is_some());
    let viewport = super::controls_fixture::canvas(&h);
    // The right edge of the sketch becomes the back edge after the placement.
    let point = move_tool::screen_point(
        viewport,
        h.state().camera.matrix(viewport.aspect_ratio()),
        glam::dvec3(30., 30., 20.),
    )
    .unwrap();
    h.event(egui::Event::PointerMoved(point));
    h.step();
    assert!(
        matches!(h.state().hover_pick, Some(Pick::Edge { ref part, .. }) if part == "housing"),
        "The pick tool must highlight the surface edge under the pointer."
    );
    super::controls_fixture::click(
        &mut h,
        point,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    button(&mut h, "Apply feature");
    wait(&mut h, |h| h.state().data.revision > rev);
    button(&mut h, "extend_1");
    wait(&mut h, |h| h.query_by_label("Area 500.00 mm²").is_some());
}

#[test]
fn stale_surface_keeps_its_pixels_and_camera_until_the_next_placed_result_arrives() {
    let (dir, mut h) = fixture();
    patch(&mut h);
    let camera = h.state().camera.matrix(1.);
    let btn = h.get_by_label("Edit surface").rect();
    let rect = super::controls_fixture::canvas(&h);
    let first = h.render().unwrap();
    let (tx, rx) = mpsc::channel();
    h.state_mut().intermediate.as_mut().unwrap().gate = Some(rx);
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut part = p.data.parts["housing"].clone();
    part.placement.translation = [8., 0., 0.].map(faber_model::Scalar::Number);
    p.put_part("housing".into(), part).unwrap();
    let rev = p.data.revision;
    drop(p);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| h.state().data.revision == rev);
    for _ in 0..8 {
        h.step();
        let b = h.get_by_label("Edit surface");
        assert_eq!(b.rect(), btn);
        assert!(b.accesskit_node().is_disabled());
        assert_eq!(camera, h.state().camera.matrix(1.));
        h.get_by_label("4 open edges");
    }
    let stale = h.render().unwrap();
    for y in rect.top() as u32 + 60..rect.bottom() as u32 - 60 {
        for x in rect.left() as u32 + 60..rect.right() as u32 - 150 {
            assert_eq!(
                first.get_pixel(x, y),
                stale.get_pixel(x, y),
                "Moved at {x},{y} before the surface arrived."
            );
        }
    }
    tx.send(()).unwrap();
    wait(&mut h, |h| {
        !h.get_by_label("Edit surface")
            .accesskit_node()
            .is_disabled()
    });
    assert_eq!(camera, h.state().camera.matrix(1.));
    let now = h.render().unwrap();
    assert_ne!(first, now);
}

#[test]
fn generated_instance_shows_its_source_surface_in_occurrence_coordinates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    std::fs::write(root.join("design.jsonnet"),r#"local f=import 'faber.libsonnet'; {components:{station:f.component('Station',translation=[60,0,0])},parts:{
      house:f.part('Housing',[f.box('block',[20,20,10]),f.feature('rim','sketch',{profile:{outer:{kind:'rectangle',origin:[0,0],size:[20,20]}},placement:{translation:[0,0,14],rotation:[0,0,0]}}),f.patch('skin',[{kind:'curve',feature:'rim'}])],'block'),
      copy:f.instance('Second housing','house',component='station')
    }}"#).unwrap();
    faber_wiring::design_source().build(&root).unwrap();
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 900.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    h.state_mut().select_only(Some("copy".into()));
    h.state_mut().camera.set_view(faber_scene::View::Top);
    h.state_mut().dirty = true;
    h.step();
    button(&mut h, "Inspect source surfaces");
    button(&mut h, "skin");
    wait(&mut h, |h| h.query_by_label("4 open edges").is_some());
    assert!(
        h.get_by_label("Edit surface")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_label("Edit the surface in design.jsonnet, then build the project.");
    let rect = super::controls_fixture::canvas(&h);
    let mat = h.state().camera.matrix(rect.aspect_ratio());
    let img = h.render().unwrap();
    for (point, blue) in [
        (glam::dvec3(10., 10., 10.), false),
        (glam::dvec3(70., 10., 14.), true),
    ] {
        let xy = move_tool::screen_point(rect, mat, point).unwrap();
        let c = img.get_pixel(xy.x as u32, xy.y as u32).0;
        assert_eq!(
            i16::from(c[2]) - i16::from(c[0]) > 25,
            blue,
            "Unexpected colour {c:?} at {point}"
        );
    }
}

#[test]
fn surface_inputs_follow_placed_profiles_paths_and_suppressed_modifiers() {
    let (dir, mut h) = fixture();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    let mut part = p.data.parts["housing"].clone();
    let faber_model::Geometry::History { features, .. } = &mut part.geometry else {
        unreachable!()
    };
    for f in [
        json!({"id":"moved_rim","geometry":{"op":"transform","source":"rim","placement":{"translation":[2,0,0],"rotation":[0,0,10]}}}),
        json!({"id":"scaled_rim","geometry":{"op":"scale","source":"moved_rim","factor":2,"about":[0,0,15]}}),
        json!({"id":"origin","geometry":{"op":"point","position":[1,0,0]}}),
        json!({"id":"placed_guide","geometry":{"op":"place_at_point","source":"guide","point":"origin"}}),
        json!({"id":"path_alias","suppressed":true,"geometry":{"op":"scale","source":"placed_guide","factor":2,"about":[0,0,0]}}),
    ] {
        features.push(serde_json::from_value(f).unwrap());
    }
    p.put_part("housing".into(), part).unwrap();
    let rev = p.data.revision;
    drop(p);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(1);
    wait(&mut h, |h| h.state().data.revision == rev);
    for (op, label, ids, key) in [
        (
            "Surface extrude",
            "Profile",
            &["moved_rim", "scaled_rim"][..],
            "profile",
        ),
        (
            "Surface sweep",
            "Sweep path",
            &["placed_guide", "path_alias"][..],
            "path",
        ),
    ] {
        button(&mut h, "Surface tools");
        button(&mut h, op);
        let combo = h.get_by_role_and_label(egui::Role::ComboBox, label);
        let previous: Vec<_> = ids
            .iter()
            .map(|id| h.query_all_by_label(id).count())
            .collect();
        combo.click();
        for _ in 0..3 {
            h.step();
        }
        for (id, before) in ids.iter().zip(previous) {
            assert!(
                h.query_all_by_label(id).count() > before,
                "The {label} list does not contain {id}."
            );
            let choice = h.query_all_by_label(id).last();
            if id == ids.last().unwrap() {
                choice.unwrap().click();
            }
        }
        h.step();
        assert_eq!(
            serde_json::to_value(
                h.state()
                    .feature_draft
                    .as_ref()
                    .unwrap()
                    .geometry()
                    .unwrap()
            )
            .unwrap()[key],
            *ids.last().unwrap()
        );
        button(&mut h, "Cancel feature");
    }
}
