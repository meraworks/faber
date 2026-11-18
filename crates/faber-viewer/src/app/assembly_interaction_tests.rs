use super::*;
use egui_kittest::{Harness, kittest::Queryable};

/// Exercise the same loaded assembly for the full drag and its return.
#[test]
fn section_drag_keeps_the_camera_and_main_mesh_during_an_assembly_inspection() {
    let root = std::env::var_os("FABER_UI_TEST_ASSEMBLY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion")
        });
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let body = bodies.iter().find(|b| b.id == "input").unwrap_or_else(|| {
        bodies
            .iter()
            .max_by(|a, b| a.volume.total_cmp(&b.volume))
            .unwrap()
    });
    let offset = (body.bounds[0][0] + body.bounds[1][0]) * 0.5;
    let plane = Section {
        normal: [1., 0., 0.],
        offset,
        reverse: false,
    };
    let name = body.name.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440., 920.))
        .with_step_dt(1. / 60.)
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, Some(plane)));
    for _ in 0..6 {
        h.step();
    }
    let view = h.get_by_label("Model viewport").rect();
    let projection = h.state().camera.matrix(view.aspect_ratio());
    let indices = h.state().cut_indices.clone();
    if let Some(row) = h.query_all_by_label(name.as_str()).next() {
        row.click();
    }
    for _ in 0..3 {
        h.step();
    }
    let tip = h
        .ctx
        .read_response(egui::Id::unique("section_offset_handle"))
        .unwrap()
        .rect
        .center();
    h.hover_at(tip);
    h.step();
    h.drag_at(tip);
    h.step();
    let mut frames = vec![];
    let mut away = 0.0f64;
    let mut stationary = None;
    for dx in [5., 10., 20., 30., 35., 35., 35., 30., 20., 10., 5., 0.] {
        h.hover_at(tip + egui::vec2(dx, 0.));
        let start = Instant::now();
        h.step();
        frames.push(start.elapsed().as_secs_f64() * 1000.);
        away = away.max((h.state().plane.offset - offset).abs());
        assert_eq!(h.get_by_label("Model viewport").rect(), view);
        assert_eq!(h.state().camera.matrix(view.aspect_ratio()), projection);
        assert!(std::sync::Arc::ptr_eq(&indices, &h.state().cut_indices));
        assert_eq!(h.state().cap_plane, Some(h.state().plane));
        if dx == 35. {
            let image = h.render().unwrap();
            let mut pixels = Vec::new();
            for y in ((view.top() as u32 + 4)..(view.bottom() as u32 - 4)).step_by(3) {
                for x in ((view.left() as u32 + 4)..(view.right() as u32 - 4)).step_by(3) {
                    pixels.push(image.get_pixel(x, y).0);
                }
            }
            if let Some(previous) = stationary.as_ref() {
                assert!(
                    previous == &pixels,
                    "The rendered viewport changed while the pointer stood still."
                );
            }
            stationary = Some(pixels);
        }
        assert!(h.state().error.is_none(), "{:?}", h.state().error);
    }
    assert!(
        away > 1e-3,
        "The section did not move when its arrow was dragged."
    );
    assert!((h.state().plane.offset - offset).abs() < 1e-5);
    h.event(egui::Event::PointerButton {
        pos: tip,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    for _ in 0..3 {
        h.step();
        assert!((h.state().plane.offset - offset).abs() < 1e-5);
    }
    frames.sort_by(f64::total_cmp);
    eprintln!(
        "{} bodies, drag frame median {:.2} ms, maximum {:.2} ms",
        h.state().bodies.len(),
        frames[frames.len() / 2],
        frames.last().unwrap()
    );
}
