use faber_ports::Mesh;
use faber_scene::{RectMode, mesh_in_rect};
use glam::{DMat4, Mat4};

fn triangle() -> Mesh {
    Mesh {
        vertices: vec![[-0.8, -0.8, 0.5], [0.8, -0.8, 0.5], [-0.8, 0.8, 0.5]],
        triangles: vec![[0, 1, 2]],
        normals: Vec::new(),
    }
}

#[test]
fn crossing_rectangle_uses_triangles_not_bounds() {
    let hit = |rect| {
        mesh_in_rect(
            &triangle(),
            DMat4::IDENTITY,
            Mat4::IDENTITY,
            rect,
            RectMode::Crossing,
        )
    };
    assert!(!hit([[0.2, 0.2], [0.75, 0.75]]));
    assert!(hit([[-0.5, -0.5], [-0.2, -0.2]]));
    assert!(hit([[-0.05, -1.], [0.05, 1.]]));
}

#[test]
fn window_rectangle_needs_every_vertex_inside() {
    let hit = |rect| {
        mesh_in_rect(
            &triangle(),
            DMat4::IDENTITY,
            Mat4::IDENTITY,
            rect,
            RectMode::Window,
        )
    };
    assert!(hit([[-0.9, -0.9], [0.9, 0.9]]));
    assert!(!hit([[-0.7, -0.9], [0.9, 0.9]]));
}

#[test]
fn crossing_rectangle_uses_the_visible_pose_and_depth() {
    let shifted = DMat4::from_translation(glam::DVec3::new(0.7, 0.7, 0.));
    let area = [[0.3, 0.3], [0.4, 0.4]];
    assert!(mesh_in_rect(
        &triangle(),
        shifted,
        Mat4::IDENTITY,
        area,
        RectMode::Crossing
    ));
    let behind = DMat4::from_translation(glam::DVec3::new(0.7, 0.7, -1.));
    assert!(!mesh_in_rect(
        &triangle(),
        behind,
        Mat4::IDENTITY,
        area,
        RectMode::Crossing
    ));
}
