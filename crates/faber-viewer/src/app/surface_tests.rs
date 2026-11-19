use super::controls_fixture::{canvas, centre, click, two_blocks};
use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn action(h: &mut Harness<'_, Shell>, label: &str) {
    if label == "Draft" {
        let menu = h.get_by_label("Analysis").rect().center();
        h.query_all_by_label(label)
            .min_by(|a, b| {
                a.rect()
                    .center()
                    .distance_sq(menu)
                    .total_cmp(&b.rect().center().distance_sq(menu))
            })
            .expect("Draft in the Analysis menu")
            .click();
    } else {
        h.get_by_label(label).click();
    }
    h.step();
    h.step();
}

fn select(h: &mut Harness<'_, Shell>, id: &str) {
    let p = centre(h, id);
    click(h, p, egui::PointerButton::Primary, Default::default());
}

fn wait(h: &mut Harness<'_, Shell>, label: &str) {
    let start = std::time::Instant::now();
    while h.query_by_label(label).is_none() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(8),
            "Missing {label}"
        );
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

#[test]
fn curvature_overlay_shows_a_legend() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    let view = canvas(&h);
    let camera = h.state().camera.matrix(view.aspect_ratio());
    let index = h.state().cut_indices.clone();
    action(&mut h, "Analysis");
    action(&mut h, "Curvature");
    wait(&mut h, "Mean curvature (1/mm)");
    action(&mut h, "Gauss");
    wait(&mut h, "Gaussian curvature (1/mm²)");
    for _ in 0..4 {
        h.step();
        assert_eq!(canvas(&h), view);
        assert_eq!(h.state().camera.matrix(view.aspect_ratio()), camera);
        assert!(std::sync::Arc::ptr_eq(&index, &h.state().cut_indices));
    }
}

#[test]
fn flat_radius_legend_has_no_zero_radius() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    action(&mut h, "Analysis");
    action(&mut h, "Curvature");
    action(&mut h, "Smallest radius");
    wait(&mut h, "Smallest radius (mm)");
    assert!(
        h.query_by_label("0.000").is_none(),
        "A plane has an infinite radius"
    );
    assert!(h.query_by_label("Infinite radius").is_some());
}

#[test]
fn draft_overlay_colours_a_vertical_face_yellow() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    action(&mut h, "Analysis");
    action(&mut h, "Draft");
    action(&mut h, "Pull X");
    wait(&mut h, "Draft result");
    let p = centre(&h, "left");
    h.hover_at(canvas(&h).left_top());
    h.step();
    let rgba = h.render().unwrap().get_pixel(p.x as u32, p.y as u32).0;
    assert!(
        rgba[0] > 170 && rgba[1] > 130 && rgba[2] < 110,
        "Expected a yellow vertical face, got {rgba:?}"
    );
}

#[test]
fn overlay_clears_after_a_geometry_change() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    action(&mut h, "Analysis");
    action(&mut h, "Access");
    wait(&mut h, "Access result");
    let old = h.state().bodies.clone();
    h.state_mut().bodies = std::sync::Arc::new(old.as_ref().clone());
    for _ in 0..3 {
        h.step();
    }
    assert!(h.query_by_label("Access result").is_none());
    action(&mut h, "Analysis");
    action(&mut h, "Zebra");
    wait(&mut h, "Stripe count");
    select(&mut h, "right");
    assert!(h.query_by_label("Stripe count").is_none());
}

#[test]
fn centre_of_mass_marker_follows_the_selected_body_and_clears_on_change() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    let p = centre(&h, "left");
    h.hover_at(canvas(&h).left_top());
    h.step();
    let baseline = h.render().unwrap();
    action(&mut h, "Analysis");
    action(&mut h, "Centre of mass");
    wait(&mut h, "Mass centre (mm)");
    let image = h.render().unwrap();
    let mut changed = 0;
    for x in p.x as u32 - 10..p.x as u32 + 10 {
        for y in p.y as u32 - 10..p.y as u32 + 10 {
            changed += usize::from(image.get_pixel(x, y) != baseline.get_pixel(x, y));
        }
    }
    assert!(
        changed > 45,
        "Only {changed} marker pixels at the placed centre"
    );
    select(&mut h, "right");
    assert!(h.query_by_label("Mass centre (mm)").is_none());
}

#[test]
fn moving_a_scene_blocker_updates_access_without_rebuilding_geometry() {
    let (_dir, mut h) = two_blocks();
    select(&mut h, "left");
    action(&mut h, "Analysis");
    action(&mut h, "Access");
    action(&mut h, "Include other parts");
    wait(&mut h, "Reachable · 225.00 mm²");
    let indices = h.state().cut_indices.clone();
    h.state_mut().motion.poses.insert(
        "right".into(),
        glam::Mat4::from_translation(glam::vec3(-60., 0., 20.)),
    );
    h.state_mut().dirty = true;
    wait(&mut h, "Reachable · 0.00 mm²");
    assert!(std::sync::Arc::ptr_eq(&indices, &h.state().cut_indices));
    h.state_mut().motion.poses.clear();
    h.state_mut().dirty = true;
    wait(&mut h, "Reachable · 225.00 mm²");
}
