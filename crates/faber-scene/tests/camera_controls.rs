use faber_scene::{Camera, View};
use glam::Vec3;

#[test]
fn iso_sees_three_faces_at_equal_angles() {
    let mut camera = Camera::fit(&[]);
    camera.set_view(View::Iso);
    let direction = (camera.eye() - camera.target).normalize();
    let expected = Vec3::new(1., -1., 1.).normalize();
    assert!((direction - expected).length() < 1e-6, "{direction:?}");
}

#[test]
fn zoom_keeps_the_point_under_the_cursor() {
    let mut camera = Camera::fit(&[]);
    let (aspect, ndc) = (1.6, [0.6, -0.4]);
    let (origin, ray) = camera.ray(aspect, ndc);
    let forward = (camera.target - camera.eye()).normalize().as_dvec3();
    let t = (camera.target.as_dvec3() - origin).dot(forward) / ray.dot(forward);
    let anchor = (origin + t * ray).as_vec3();
    camera.zoom_at(aspect, ndc, 0.4);
    let p = camera.matrix(aspect).project_point3(anchor);
    assert!((p.x - ndc[0]).abs() * 800. < 0.5);
    assert!((p.y - ndc[1]).abs() * 500. < 0.5);
}

#[test]
fn edges_and_corners_give_the_requested_eye_direction() {
    for (view, expected) in [
        (View::Edge([0, -1, 1]), Vec3::new(0., -1., 1.)),
        (View::Corner([-1, 1, -1]), Vec3::new(-1., 1., -1.)),
    ] {
        let mut camera = Camera::fit(&[]);
        camera.set_view(view);
        assert!(
            ((camera.eye() - camera.target).normalize() - expected.normalize()).length() < 1e-6
        );
    }
}

#[test]
fn top_and_bottom_pan_use_the_screen_axes() {
    let mut camera = Camera::fit(&[]);
    camera.target = Vec3::splat(4096.);
    camera.distance = 50.;
    for view in [View::Top, View::Bottom] {
        camera.set_view(view);
        let (right, up) = camera.screen_axes();
        let m = camera.matrix(1.6);
        let centre = m.project_point3(camera.target);
        let x = m.project_point3(camera.target + right) - centre;
        let y = m.project_point3(camera.target + up) - centre;
        assert!(x.x > 0. && x.y.abs() < 1e-4 && y.y > 0. && y.x.abs() < 1e-4);
    }
}
