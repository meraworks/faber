use faber_ports::{Body, Mesh};
use faber_scene::View;
use image::RgbaImage;
use std::collections::BTreeMap;
use std::path::PathBuf;

const SIZE: u32 = 128;
const BASE: [u8; 3] = [200, 200, 200];
const RED: [u8; 3] = [255, 0, 0];

/// A 20 mm cube. Triangles 2 and 3 form the top face.
fn cube() -> Body {
    let vertices: Vec<[f64; 3]> = (0..8)
        .map(|i| {
            [
                (i & 1) as f64 * 20.,
                ((i >> 1) & 1) as f64 * 20.,
                ((i >> 2) & 1) as f64 * 20.,
            ]
        })
        .collect();
    let triangles = vec![
        [0, 1, 3],
        [0, 3, 2],
        [4, 5, 7],
        [4, 7, 6],
        [0, 1, 5],
        [0, 5, 4],
        [2, 3, 7],
        [2, 7, 6],
        [0, 2, 6],
        [0, 6, 4],
        [1, 3, 7],
        [1, 7, 5],
    ];
    Body {
        id: "cube".into(),
        name: "Cube".into(),
        group: String::new(),
        colour: BASE,
        mesh: Mesh {
            vertices,
            triangles,
            normals: Vec::new(),
        },
        volume: 8000.,
        bounds: [[0., 0., 0.], [20., 20., 20.]],
        face_of_triangle: Vec::new(),
        edge_lines: Default::default(),
        shape: None,
    }
}

fn render(paint: &BTreeMap<String, Vec<[u8; 3]>>, name: &str) -> RgbaImage {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    faber_render::save_painted(&[cube()], &path, None, paint, SIZE, SIZE, View::Top).unwrap();
    image::open(&path).unwrap().to_rgba8()
}

#[test]
fn painted_triangles_change_only_their_own_faces() {
    let mut triangles = vec![BASE; 12];
    triangles[2] = RED;
    triangles[3] = RED;
    let mut paint = BTreeMap::new();
    paint.insert("cube".to_string(), triangles);

    let centre = render(&paint, "overhang-top-paint.png")
        .get_pixel(SIZE / 2, SIZE / 2)
        .0;
    assert!(
        centre[0] > 120 && centre[1] < 60 && centre[2] < 60,
        "{centre:?}"
    );

    let plain = render(&BTreeMap::new(), "overhang-top-plain.png")
        .get_pixel(SIZE / 2, SIZE / 2)
        .0;
    assert!(
        plain[0] > 100 && plain[1] > 100 && plain[2] > 100,
        "{plain:?}"
    );
}
