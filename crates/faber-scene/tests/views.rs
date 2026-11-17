use faber_scene::{Camera, View};
use glam::Vec3;

#[test]
fn axial_views_look_towards_the_target_without_a_singular_matrix() {
    for (view, outward) in [
        (View::Front, -Vec3::Y),
        (View::Back, Vec3::Y),
        (View::Left, -Vec3::X),
        (View::Right, Vec3::X),
        (View::Top, Vec3::Z),
        (View::Bottom, -Vec3::Z),
    ] {
        let mut camera = Camera::fit(&[]);
        camera.set_view(view);
        let direction = (camera.eye() - camera.target).normalize();
        assert!(direction.distance(outward) < 1e-6, "{view:?}");
        assert!(camera.matrix(1.5).is_finite(), "{view:?}");
        assert!(camera.matrix(1.5).inverse().is_finite(), "{view:?}");
        let (_, ray) = camera.ray(1.5, [0., 0.]);
        assert!(ray.as_vec3().distance(-outward) < 0.001, "{view:?}");
    }
}
