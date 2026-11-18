use faber_ports::Mesh;
use faber_scene::{Section, pick_triangle};
use glam::DVec3;

#[test]
fn pick_triangle_returns_the_nearest_triangle() {
    let mesh = Mesh {
        vertices: vec![
            [0., 0., 5.],
            [5., 0., 5.],
            [0., 5., 5.],
            [0., 0., 1.],
            [5., 0., 1.],
            [0., 5., 1.],
        ],
        triangles: vec![[3, 4, 5], [0, 1, 2]],
        normals: vec![],
    };
    let eye = DVec3::new(1., 1., 10.);
    assert_eq!(pick_triangle(&mesh, eye, -DVec3::Z, None), Some((5., 1)));
    let plane = Section {
        normal: [0., 0., 1.],
        offset: 4.,
        reverse: false,
    };
    assert_eq!(
        pick_triangle(&mesh, eye, -DVec3::Z, Some(plane)),
        Some((9., 0))
    );
}
