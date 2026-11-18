use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use std::fmt::Write;

/// A 10 mm cube with both triangles on the top removed.
fn broken_cube() -> String {
    let mut obj = String::new();
    for [x, y, z] in [
        [0, 0, 0],
        [10, 0, 0],
        [10, 10, 0],
        [0, 10, 0],
        [0, 0, 10],
        [10, 0, 10],
        [10, 10, 10],
        [0, 10, 10],
    ] {
        writeln!(obj, "v {x} {y} {z}").unwrap();
    }
    for [a, b, c, d] in [
        [1, 4, 3, 2],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 4, 8, 7],
        [4, 1, 5, 8],
    ] {
        writeln!(obj, "f {a} {b} {c}\nf {a} {c} {d}").unwrap();
    }
    obj
}
fn fixture(purchased: bool) -> (tempfile::TempDir, Harness<'static, Shell>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("open.obj");
    std::fs::write(&path, broken_cube()).unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Mesh fixture".into())).unwrap();
    project
        .import_with_record(
            "cube".into(),
            "Open cube".into(),
            &path,
            false,
            purchased.then(|| faber_model::Purchased {
                source: "Local test".into(),
                licence: "CC0".into(),
                designation: "Mesh fixture".into(),
                mass_g: None,
            }),
        )
        .unwrap();
    let data = project.data.clone();
    let bodies = project.bodies.clone();
    drop(project);
    let root = dir.path().to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 960.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    for _ in 0..4 {
        h.step();
    }
    (dir, h)
}
fn menu(h: &mut Harness<'_, Shell>) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Open cube")
        .click_secondary();
    for _ in 0..4 {
        h.step();
    }
}
fn wait(h: &mut Harness<'_, Shell>, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while h.query_all_by_label(label).next().is_none() {
        h.step();
        assert!(
            Instant::now() < deadline,
            "Missing {label}. App error: {:?}",
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(8));
    }
}
#[test]
fn mesh_tools_report_and_repair_from_the_context_menu() {
    let (dir, mut h) = fixture(false);
    let before = h.state().data.clone();
    menu(&mut h);
    for name in [
        "Mesh report",
        "Repair",
        "Reduce",
        "Plane cut",
        "Convert to solid",
    ] {
        assert!(
            h.query_by_role_and_label(egui::accesskit::Role::Button, name)
                .is_some(),
            "Missing {name}"
        );
    }
    h.get_by_label("Mesh report").click();
    wait(&mut h, "4 open edges");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Repair")
        .click();
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Apply repair").click();
    wait(&mut h, "Watertight");
    let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
    assert_eq!(project.data.revision, before.revision + 1);
    assert_ne!(
        project.data.parts["cube"].geometry,
        before.parts["cube"].geometry
    );
    project.undo().unwrap();
    assert_eq!(
        project.data.parts["cube"].geometry,
        before.parts["cube"].geometry
    );
}
#[test]
fn mesh_tools_protect_a_purchased_mesh() {
    let (_dir, mut h) = fixture(true);
    menu(&mut h);
    for name in ["Repair", "Reduce", "Plane cut", "Convert to solid"] {
        assert!(
            h.get_by_role_and_label(egui::accesskit::Role::Button, name)
                .accesskit_node()
                .is_disabled(),
            "{name}"
        );
    }
    h.get_by_label("Mesh report").click();
    wait(&mut h, "4 open edges");
}
#[test]
fn mesh_tools_example_uses_the_generated_broken_cube() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mesh-repair");
    let (data, _) = faber_wiring::read(&example).unwrap();
    let faber_model::Geometry::Imported { asset, .. } = &data.parts["cube"].geometry else {
        panic!()
    };
    assert_eq!(
        std::fs::read(example.join("assets").join(asset)).unwrap(),
        broken_cube().as_bytes()
    );
}

#[test]
fn mesh_tools_refuse_a_stale_repair_form() {
    let (dir, mut h) = fixture(false);
    menu(&mut h);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Repair")
        .click();
    wait(&mut h, "4 open edges");
    let mut other = faber_wiring::open(dir.path(), false, None).unwrap();
    other
        .set_params([("wall".into(), "2".into())].into())
        .unwrap();
    let latest = other.data.clone();
    drop(other);
    h.state_mut().last_poll = Instant::now();
    h.get_by_label("Apply repair").click();
    wait(
        &mut h,
        "The project changed. Reopen Mesh tools for the current revision.",
    );
    let (saved, _) = faber_wiring::read(dir.path()).unwrap();
    assert_eq!(saved, latest);
}

fn wait_revision(h: &mut Harness<'_, Shell>, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while h.state().data.revision != expected || h.state().pending.is_some() {
        h.step();
        assert!(
            Instant::now() < deadline,
            "Revision {} did not advance to {expected}: {:?}",
            h.state().data.revision,
            h.state().error
        );
        std::thread::sleep(Duration::from_millis(8));
    }
    for _ in 0..3 {
        h.step();
    }
}
#[test]
fn mesh_tools_cut_both_sides_and_convert_the_copy() {
    let (dir, mut h) = fixture(false);
    let revision = h.state().data.revision;
    menu(&mut h);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Repair")
        .click();
    wait(&mut h, "4 open edges");
    h.get_by_label("Apply repair").click();
    wait_revision(&mut h, revision + 1);
    h.get_by_label("Plane cut").click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("Both").click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("New part id for the upper side").focus();
    h.step();
    h.get_by_label("New part id for the upper side")
        .type_text("top");
    h.step();
    h.get_by_label("Apply plane cut").click();
    wait_revision(&mut h, revision + 2);
    assert_eq!(h.state().selected.as_deref(), Some("top"));
    h.get_by_label("Convert to solid").click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("Create solid").click();
    wait_revision(&mut h, revision + 3);
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    let solid = bodies.iter().find(|b| b.id == "top").unwrap();
    assert!(solid.shape.is_some());
    assert!((solid.volume - 500.).abs() < 1e-7);
    assert!(
        bodies
            .iter()
            .find(|b| b.id == "cube")
            .unwrap()
            .shape
            .is_none()
    );
}

/// Track the viewport, camera projection and mesh action row in each rendered frame.
#[test]
fn mesh_worker_frames_keep_the_view_and_controls_still() {
    for width in [1440., 860.] {
        let (_dir, mut h) = fixture(false);
        h.set_size(egui::vec2(width, 680.));
        for _ in 0..8 {
            h.step();
        }
        menu(&mut h);
        let initial_view = super::controls_fixture::canvas(&h);
        let projection = h.state().camera.matrix(initial_view.aspect_ratio());
        let mut action_positions: Vec<egui::Pos2> = Vec::new();
        let mut read_busy = false;
        let mut repair_busy = false;
        let mut capture = |h: &Harness<'_, Shell>, repairing: bool| {
            let view = super::controls_fixture::canvas(h);
            assert!(
                (view.min - initial_view.min).length() <= 0.5
                    && (view.max - initial_view.max).length() <= 0.5,
                "{width} pt: viewport changed from {initial_view:?} to {view:?}"
            );
            assert_eq!(
                h.state().camera.matrix(view.aspect_ratio()),
                projection,
                "{width} pt: camera moved"
            );
            if h.query_all_by_label("Close mesh tools").next().is_some() {
                let tab = h
                    .query_all_by_label("Reduce")
                    .map(|n| n.rect().center())
                    .filter(|p| p.x >= view.right())
                    .max_by(|a, b| a.x.total_cmp(&b.x))
                    .expect("Mesh action row");
                action_positions.push(tab);
                let busy = h.query_all_by_label("Working on mesh…").next().is_some();
                if repairing {
                    repair_busy |= busy;
                } else {
                    read_busy |= busy;
                }
            }
        };
        h.get_by_label("Mesh report").click();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            h.step();
            capture(&h, false);
            if h.query_all_by_label("4 open edges").next().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "Report did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        for _ in 0..5 {
            h.step();
            capture(&h, false);
        }
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Repair")
            .click();
        for _ in 0..3 {
            h.step();
            capture(&h, false);
        }
        h.get_by_label("Apply repair").click();
        loop {
            h.step();
            capture(&h, true);
            if h.query_all_by_label("Watertight").next().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "Repair did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        for _ in 0..5 {
            h.step();
            capture(&h, true);
        }
        assert!(
            read_busy && repair_busy,
            "Capture both worker transitions at {width} pt"
        );
        let initial = action_positions[0];
        let delta = action_positions
            .iter()
            .map(|p| p.distance(initial))
            .fold(0.0f32, f32::max);
        eprintln!(
            "{width} pt, {} frames: maximum mesh action displacement {delta:.2} pt",
            action_positions.len()
        );
        assert!(
            delta <= 0.5,
            "{width} pt: mesh action row moved {delta:.2} pt from {initial:?} over {} frames",
            action_positions.len()
        );
    }
}
