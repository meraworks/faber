use faber_ports::{Body, Mesh};
use faber_scene::View;

#[test]
fn dark_surfaces_keep_their_value_under_direct_reflection() {
    let normal = (glam::DVec3::new(-0.4, -0.7, 1.).normalize() + glam::DVec3::X)
        .normalize()
        .to_array();
    let body = Body {
        id: "face".into(),
        name: "Face".into(),
        group: String::new(),
        colour: [40, 43, 48],
        volume: 0.,
        shape: None,
        bounds: [[0., -10., -10.], [0., 10., 10.]],
        mesh: Mesh {
            vertices: vec![
                [0., -10., -10.],
                [0., 10., -10.],
                [0., 10., 10.],
                [0., -10., 10.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            normals: vec![normal; 4],
        },
    };
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("dark-face.png");
    faber_render::save_view(&[body], &path, None, 128, 128, View::Right).unwrap();
    let image = image::open(path).unwrap().to_rgba8();
    let red = image.get_pixel(64, 64)[0];
    assert!((30..=65).contains(&red), "Dark face red channel: {red}");
}
