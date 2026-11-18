use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

pub(super) fn two_blocks() -> (tempfile::TempDir, Harness<'static, Shell>) {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Controls".into())).unwrap();
    for (id, x) in [("left", -30), ("right", 30)] {
        let part: faber_model::Part = serde_json::from_value(json!({
            "name": id, "colour":[100,130,160],
            "placement":{"translation":[x,0,0],"rotation":[0,0,0]},
            "geometry":{"op":"history","output":"block","features":[
                {"id":"block","geometry":{"op":"box","size":[15,15,15]}}
            ]}
        }))
        .unwrap();
        project.put_part(id.into(), part).unwrap();
    }
    let data = project.data.clone();
    let bodies = project.bodies.clone();
    drop(project);
    let root = dir.path().to_owned();
    let mut h = Harness::builder()
        .with_step_dt(1. / 60.)
        .with_size(egui::vec2(1440., 900.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    h.state_mut().dirty = true;
    h.state_mut().camera.set_view(faber_scene::View::Top);
    for _ in 0..3 {
        h.step();
    }
    (dir, h)
}

pub(super) fn canvas(h: &Harness<'_, Shell>) -> egui::Rect {
    h.get_by_label("Model viewport")
        .rect()
        .expand2(Vec2::new(5., 0.))
}

pub(super) fn centre(h: &Harness<'_, Shell>, id: &str) -> egui::Pos2 {
    let rect = canvas(h);
    let b = h.state().bodies.iter().find(|b| b.id == id).unwrap();
    let centre = (glam::DVec3::from(b.bounds[0]) + glam::DVec3::from(b.bounds[1])) / 2.;
    move_tool::screen_point(rect, h.state().camera.matrix(rect.aspect_ratio()), centre).unwrap()
}

pub(super) fn click(
    h: &mut Harness<'_, Shell>,
    pos: egui::Pos2,
    button: egui::PointerButton,
    modifiers: egui::Modifiers,
) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event_modifiers(
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            },
            modifiers,
        );
        h.step();
    }
    h.step();
}
