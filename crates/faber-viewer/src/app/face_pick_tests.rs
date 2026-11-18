use super::controls_fixture::{canvas, centre, click, two_blocks};
use super::*;
use egui_kittest::kittest::Queryable;

fn screen(h: &egui_kittest::Harness<'_, Shell>, point: [f64; 3]) -> egui::Pos2 {
    let r = canvas(h);
    move_tool::screen_point(r, h.state().camera.matrix(r.aspect_ratio()), point.into()).unwrap()
}

#[test]
fn face_filter_picks_the_top_face_of_a_box() {
    let (dir, mut h) = two_blocks();
    h.get_by_label("Face").click();
    h.step();
    let p = centre(&h, "left");
    click(
        &mut h,
        p,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    let Some(Pick::Face { part, face }) = h.state().picks.last() else {
        panic!("Select the top face")
    };
    assert_eq!(part, "left");
    let project = faber_wiring::open(dir.path(), false, None).unwrap();
    let info = project
        .topology(part)
        .unwrap()
        .1
        .into_iter()
        .find(|f| f.index == *face)
        .unwrap();
    assert_eq!(info.normal, Some([0., 0., 1.]));
    assert!((h.state().pick_point.unwrap()[2] - 15.).abs() < 1e-5);
    let right = centre(&h, "right");
    click(
        &mut h,
        right,
        egui::PointerButton::Primary,
        egui::Modifiers::SHIFT,
    );
    assert_eq!(h.state().picks.len(), 2);
    click(
        &mut h,
        p,
        egui::PointerButton::Primary,
        egui::Modifiers::CTRL,
    );
    assert_eq!(h.state().picks.len(), 1);
    assert!(matches!(&h.state().picks[0],Pick::Face{part,..} if part=="right"));
}

#[test]
fn face_hover_tints_only_that_face() {
    let (_d, mut h) = two_blocks();
    h.state_mut().camera.set_view(faber_scene::View::Iso);
    h.state_mut().dirty = true;
    h.state_mut().set_filter(SelectFilter::Face);
    h.step();
    let top = screen(&h, [-22.5, 7.5, 15.]);
    let side = screen(&h, [-15., 7.5, 7.5]);
    let colour = |h: &egui_kittest::Harness<'_, Shell>, pos: egui::Pos2| {
        let r = canvas(h);
        let target = h.state().target.as_ref().unwrap();
        let x = ((pos.x - r.left()) / r.width() * target.width as f32) as usize;
        let y = ((pos.y - r.top()) / r.height() * target.height as f32) as usize;
        let offset = (y * target.width as usize + x) * 4;
        h.state().gpu.read(target).unwrap()[offset..offset + 3].to_vec()
    };
    let before = [colour(&h, top), colour(&h, side)];
    h.hover_at(top);
    h.step();
    h.step();
    assert_ne!(colour(&h, top), before[0]);
    assert_eq!(colour(&h, side), before[1]);
}

#[test]
fn mesh_part_is_picked_as_a_body() {
    let (_d, mut h) = two_blocks();
    let bodies = std::sync::Arc::make_mut(&mut h.state_mut().bodies);
    let part = bodies.iter_mut().find(|b| b.id == "left").unwrap();
    part.face_of_triangle.clear();
    part.shape = None;
    h.state_mut().set_filter(SelectFilter::Face);
    let p = centre(&h, "left");
    click(
        &mut h,
        p,
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert_eq!(h.state().picks, vec![Pick::Body("left".into())]);
}

#[test]
fn pick_ignores_the_cut_away_half() {
    let (_d, mut h) = two_blocks();
    h.state_mut().set_filter(SelectFilter::Face);
    h.state_mut().section_on = true;
    h.state_mut().plane = Section {
        normal: [0., 0., 1.],
        offset: 9.,
        reverse: false,
    };
    let p = centre(&h, "left");
    let (picked, point) = h.state().pick_entity(canvas(&h), p).unwrap();
    assert!(matches!(picked, Pick::Face { .. }));
    assert!(point[2].abs() < 1e-6);
}

#[test]
fn saved_section_leaves_a_front_nonmember_visible_and_pickable() {
    let (_dir, mut h) = two_blocks();
    let ctx = h.ctx.clone();
    let mut front = h.state().data.parts["right"].clone();
    front.placement.translation = [-30., 0., 7.].map(faber_model::Scalar::Number);
    let plane = Section {
        normal: [0., 0., 1.],
        offset: 9.,
        reverse: false,
    };
    let saved = |id: &str| {
        let mut section = crate::analysis::from_plane(plane);
        section.parts.push(id.into());
        faber_model::Edit::SetSection {
            id: id.into(),
            section,
        }
    };
    h.state_mut().save_sections(
        &ctx,
        vec![
            faber_model::Edit::PutPart {
                id: "right".into(),
                part: front,
            },
            saved("left"),
            saved("right"),
        ],
    );
    let until = Instant::now() + Duration::from_secs(10);
    while h.state().pending.is_some() {
        h.step();
        assert!(Instant::now() < until, "Scene did not load");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(h.state().error, None);
    let pos = screen(&h, [-22.5, 7.5, 22.]);
    let rect = canvas(&h);
    let colour = |s: &Shell| {
        let t = s.target.as_ref().unwrap();
        let x = ((pos.x - rect.left()) / rect.width() * t.width as f32) as usize;
        let y = ((pos.y - rect.top()) / rect.height() * t.height as f32) as usize;
        let i = (y * t.width as usize + x) * 4;
        s.gpu.read(t).unwrap()[i..i + 3].to_vec()
    };
    let top = colour(h.state());
    h.state_mut().section_selected = Some("left".into());
    h.state_mut().sync_section(&ctx);
    h.step();
    assert_eq!(
        colour(h.state()),
        top,
        "The top face of the nonmember changed"
    );
    h.state_mut().set_filter(SelectFilter::Face);
    let (pick, point) = h.state().pick_entity(rect, pos).unwrap();
    assert_eq!(pick.part(), "right");
    assert!((point[2] - 22.).abs() < 1e-5);
    assert!(h.state().caps.as_ref().unwrap().cap_mesh("right").is_none());
    h.state_mut().hidden.insert("right".into());
    h.state_mut().mesh_dirty = true;
    h.step();
    let (id, point) = h.state().pick_surface(rect, pos).unwrap();
    assert_eq!(id, "left");
    assert!((point.z - 9.).abs() < 1e-5);
    assert_ne!(colour(h.state()), top);
    h.state_mut().hidden.remove("right");
    h.state_mut().mesh_dirty = true;
    h.step();
    assert_eq!(colour(h.state()), top);
    h.state_mut().section_selected = Some("right".into());
    h.state_mut().sync_section(&ctx);
    h.step();
    let caps = h.state().caps.as_ref().unwrap();
    assert!(caps.cap_mesh("left").is_none() && caps.cap_mesh("right").is_some());
    assert_eq!(h.state().pick_surface(rect, pos).unwrap().0, "left");
}
