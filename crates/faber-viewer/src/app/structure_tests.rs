use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::{Value, json};

fn fixture() -> (tempfile::TempDir, Harness<'static, Shell>) {
    assembly_fixture(true, None)
}

fn assembly_fixture(
    grouped: bool,
    section: Option<Section>,
) -> (tempfile::TempDir, Harness<'static, Shell>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    let mut model = json!({"schema":1,"revision":1,"name":"Assembly test","params":{"width":"20"},
        "components":{"frame":{"name":"Frame"},"left":{"name":"Left arm","parent":"frame","placement":{"translation":[40,0,0],"rotation":[0,0,0]}}},
        "configurations":{"short":{"params":{"width":"20"}},"wide":{"params":{"width":"65"}}},
        "parts":{
          "bar":{"name":"Crossbar","material":"steel-s235","colour":[70,120,110],"geometry":{"op":"box","size":["width",4,5]},"component":"left"},
          "foot":{"name":"Loose foot","colour":[100,110,120],"geometry":{"op":"box","size":[8,8,8]},
            "placement":{"translation":[-35,0,0],"rotation":[0,0,0]}}}});
    if !grouped {
        model["components"] = json!({});
        model["parts"]["bar"]
            .as_object_mut()
            .unwrap()
            .remove("component");
    }
    std::fs::write(root.join("faber.json"), serde_json::to_vec(&model).unwrap()).unwrap();
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 900.))
        .with_step_dt(1. / 60.)
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, section));
    for _ in 0..5 {
        h.step();
    }
    (dir, h)
}
fn button(h: &Harness<'_, Shell>, label: &str) -> egui::Rect {
    h.get_by_role_and_label(egui::Role::Button, label).rect()
}
fn click(h: &mut Harness<'_, Shell>, label: &str) {
    h.get_by_role_and_label(egui::Role::Button, label).click();
    for _ in 0..4 {
        h.step();
    }
}
fn wait(h: &mut Harness<'_, Shell>, condition: impl Fn(&Shell) -> bool) {
    let due = Instant::now() + Duration::from_secs(10);
    while !condition(h.state()) {
        h.step();
        assert!(
            Instant::now() < due,
            "Operation did not finish: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}
fn choose_configuration(h: &mut Harness<'_, Shell>, name: &str) {
    h.get_by_role_and_label(egui::Role::ComboBox, "Configuration")
        .click();
    for _ in 0..4 {
        h.step();
    }
    click(h, name);
}
#[test]
fn browser_shows_the_tree() {
    let (_dir, mut h) = fixture();
    h.get_by_label("Components");
    let parent = button(&h, "Frame");
    let child = button(&h, "Left arm");
    let body = button(&h, "Crossbar");
    assert!(child.left() > parent.left() && body.left() > child.left());
    click(&mut h, "Crossbar");
    assert_eq!(h.state().selection(), ["bar"]);
    let canvas = h.get_by_label("Model viewport").rect();
    for _ in 0..8 {
        h.step();
        assert_eq!(canvas, h.get_by_label("Model viewport").rect());
        assert_eq!(body, button(&h, "Crossbar"));
    }
}
#[test]
fn isolate_hides_the_other_parts() {
    let (_dir, mut h) = fixture();
    h.state_mut().hidden.insert("bar".into());
    click(&mut h, "Frame");
    click(&mut h, "Isolate");
    assert!(h.state().hidden.contains("foot"));
    assert!(!h.state().hidden.contains("bar"));
    click(&mut h, "End isolate");
    assert!(h.state().hidden.contains("bar"));
    assert!(!h.state().hidden.contains("foot"));
}

#[test]
fn flat_and_tree_visibility_checkboxes_update_section_pixels_and_reuse_the_mesh() {
    for grouped in [false, true] {
        let plane = Section {
            normal: [0., 0., 1.],
            offset: 2.5,
            reverse: false,
        };
        let (_dir, mut h) = assembly_fixture(grouped, Some(plane));
        let view = h.get_by_label("Model viewport").rect();
        let camera = h.state().camera.matrix(view.aspect_ratio());
        let indices = h.state().cut_indices.clone();
        let bodies = h.state().bodies.clone();
        let pixels = |h: &mut Harness<'_, Shell>| {
            let image = h.render().unwrap();
            let mut result = Vec::new();
            for y in ((view.top() as u32 + 22)..(view.bottom() as u32 - 22)).step_by(2) {
                for x in ((view.left() as u32 + 14)..(view.right() as u32 - 14)).step_by(2) {
                    result.push(image.get_pixel(x, y).0);
                }
            }
            result
        };
        assert!(
            h.state()
                .caps
                .as_ref()
                .and_then(|c| c.cap_mesh("bar"))
                .is_some()
        );
        let original = pixels(&mut h);
        for hidden in [true, false] {
            h.get_by_role_and_label(egui::Role::CheckBox, "Show Crossbar")
                .click();
            for _ in 0..4 {
                h.step();
                assert_eq!(h.get_by_label("Model viewport").rect(), view);
                assert_eq!(h.state().camera.matrix(view.aspect_ratio()), camera);
                assert!(std::sync::Arc::ptr_eq(&bodies, &h.state().bodies));
                assert!(std::sync::Arc::ptr_eq(&indices, &h.state().cut_indices));
                assert_eq!(h.state().cap_plane, Some(plane));
            }
            assert_eq!(h.state().hidden.contains("bar"), hidden);
            assert!(!h.state().hidden.contains("foot"));
            let current = pixels(&mut h);
            let changed = original
                .iter()
                .zip(&current)
                .filter(|(a, b)| a != b)
                .count();
            if hidden {
                assert!(
                    changed > 100,
                    "Hiding the cut body changed only {changed} sampled pixels"
                );
            } else {
                assert_eq!(
                    changed, 0,
                    "Showing the cut body did not restore the original pixels"
                );
            }
        }
    }
}
#[test]
fn linked_copy_makes_an_instance() {
    let (dir, mut h) = fixture();
    click(&mut h, "Crossbar");
    click(&mut h, "Move/Copy");
    h.get_by_label("Create copy").click();
    h.step();
    h.get_by_label("Linked copy").click();
    h.step();
    click(&mut h, "OK");
    wait(&mut h, |s| s.data.parts.contains_key("bar_copy"));
    let (d, _) = faber_wiring::read(dir.path()).unwrap();
    let p = serde_json::to_value(&d.parts["bar_copy"]).unwrap();
    assert_eq!(p["geometry"], json!({"op":"instance","of":"bar"}));
    assert_eq!(p["component"], "left");
}
#[test]
fn configuration_selector_changes_the_size() {
    let (dir, mut h) = fixture();
    click(&mut h, "Parts list");
    h.get_by_label("Quantity");
    let canvas = h.get_by_label("Model viewport").rect();
    choose_configuration(&mut h, "wide");
    wait(&mut h, |s| s.data.params["width"] == "65");
    let (d, bodies) = faber_wiring::read(dir.path()).unwrap();
    let b = bodies.iter().find(|b| b.id == "bar").unwrap();
    assert!((b.bounds[1][0] - b.bounds[0][0] - 65.).abs() < 1e-6);
    assert_eq!(
        serde_json::to_value(d).unwrap()["active_configuration"],
        "wide"
    );
    for _ in 0..8 {
        h.step();
        assert_eq!(canvas, h.get_by_label("Model viewport").rect());
    }
    h.get_by_label("Quantity");
    if let Some(path) = std::env::var_os("FABER_ASSEMBLY_TEST_SNAPSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
}
#[test]
fn dropping_a_part_updates_its_component_and_keeps_its_world_position() {
    let (dir, mut h) = fixture();
    let before = h
        .state()
        .bodies
        .iter()
        .find(|b| b.id == "foot")
        .unwrap()
        .bounds;
    let from = button(&h, "Loose foot").center();
    let to = button(&h, "Left arm").center();
    h.hover_at(from);
    h.step();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    h.hover_at(from + egui::vec2(9., 0.));
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
    wait(&mut h, |s| s.data.revision > 1);
    let (d, b) = faber_wiring::read(dir.path()).unwrap();
    let p: Value = serde_json::to_value(&d.parts["foot"]).unwrap();
    assert_eq!(p["component"], "left");
    assert_eq!(b.iter().find(|b| b.id == "foot").unwrap().bounds, before);
}

#[test]
fn parts_list_refreshes_group_mass_without_moving_the_canvas_or_table_header() {
    let (dir, mut h) = fixture();
    h.set_size(egui::vec2(920., 650.));
    for _ in 0..6 {
        h.step();
    }
    click(&mut h, "Parts list");
    let mass = |width: f64| format!("{:.2}", width * 4. * 5. * 7.85 / 1000.);
    let until = Instant::now() + Duration::from_secs(10);
    while h.query_all_by_label(&mass(20.)).next().is_none() {
        h.step();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(4));
    }
    let frame = h.get_by_label("Model viewport").rect();
    let title = h.get_by_label("Quantity").rect();
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    faber_wiring::use_configuration(dir.path(), &mut project, "wide", false).unwrap();
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    let mut settled = 0;
    while settled < 6 {
        h.step();
        assert_eq!(h.get_by_label("Model viewport").rect(), frame);
        assert_eq!(
            h.get_by_label("Quantity").rect(),
            title,
            "The parts list header moved on update"
        );
        let old = h.query_all_by_label(&mass(20.)).next().is_some();
        let new = h.query_all_by_label(&mass(65.)).next().is_some();
        assert!(old || new, "Parts list rows disappeared during a rebuild");
        if new {
            settled += 1;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn long_names_and_a_stopped_bom_worker_keep_narrow_controls_in_place() {
    let (dir, mut h) = fixture();
    h.set_size(egui::vec2(920., 680.));
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    let key = "wide_supports_for_the_full_length_workbench_edition";
    let partly = project.data.configurations["wide"].clone();
    project
        .batch_at_revision(
            project.data.revision,
            vec![faber_model::Edit::SetConfiguration {
                name: key.into(),
                configuration: partly,
            }],
        )
        .unwrap();
    let long = "Crossbar with a long name that must stay within the name column of the parts list";
    let mut bar = project.data.parts["bar"].clone();
    bar.name = long.into();
    project.put_part("bar".into(), bar).unwrap();
    drop(project);
    h.state_mut().last_poll = Instant::now() - Duration::from_secs(2);
    wait(&mut h, |s| s.data.configurations.contains_key(key));
    click(&mut h, "Parts list");
    let due = Instant::now() + Duration::from_secs(8);
    while h.query_all_by_label("3.14").next().is_none() {
        h.step();
        assert!(Instant::now() < due);
        std::thread::sleep(Duration::from_millis(4));
    }
    let frame = h.get_by_label("Model viewport").rect();
    let neighbour = button(&h, "Checks");
    let column = h.get_by_role_and_label(egui::Role::Label, "Name").rect();
    let name_cell = h
        .query_all_by_role_and_label(egui::Role::Label, long)
        .map(|n| n.rect())
        .find(|r| (r.left() - column.left()).abs() < 4. && r.top() >= column.bottom())
        .expect("The part name is missing below its column heading");
    assert!(
        name_cell.width() <= 175.,
        "Name cell grew to {}",
        name_cell.width()
    );
    let held = h.state_mut().structure.hold_bom();
    choose_configuration(&mut h, key);
    wait(&mut h, |s| {
        s.data.active_configuration.as_deref() == Some(key)
    });
    for _ in 0..4 {
        h.step();
        h.get_by_label("Updating parts list…");
        assert_eq!(
            neighbour,
            button(&h, "Checks"),
            "Long configuration moved the toolbar"
        );
        assert_eq!(frame, h.get_by_label("Model viewport").rect());
    }
    drop(held);
    let due = Instant::now() + Duration::from_secs(8);
    while h.query_all_by_label("10.21").next().is_none()
        && h.query_all_by_label("10.20").next().is_none()
    {
        h.step();
        assert!(
            Instant::now() < due,
            "A stopped obsolete worker blocked the current parts list"
        );
        std::thread::sleep(Duration::from_millis(4));
    }
    let held = h.state_mut().structure.hold_bom();
    drop(held);
    h.step();
    h.get_by_label("The parts list worker stopped.");
    click(&mut h, "Retry parts list");
    for _ in 0..8 {
        h.step();
        assert_eq!(frame, h.get_by_label("Model viewport").rect());
    }
}
