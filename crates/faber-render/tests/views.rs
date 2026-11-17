use faber_ports::{Body, Mesh};
use faber_scene::View;
use image::RgbaImage;
use std::path::PathBuf;

const SIZE: u32 = 200;

/// Builds a centred box with a red colour.
fn box_body(size: [f64; 3]) -> Body {
    let [x, y, z] = size.map(|v| v / 2.);
    let vertices = vec![
        [-x, -y, -z],
        [x, -y, -z],
        [x, y, -z],
        [-x, y, -z],
        [-x, -y, z],
        [x, -y, z],
        [x, y, z],
        [-x, y, z],
    ];
    let triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    Body {
        id: "box".into(),
        name: "Box".into(),
        group: String::new(),
        colour: [240, 60, 60],
        mesh: Mesh {
            vertices,
            triangles,
        },
        volume: size.iter().product(),
        bounds: [[-x, -y, -z], [x, y, z]],
        shape: None,
    }
}

fn render(view: View) -> RgbaImage {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("box-{view:?}.png"));
    let bodies = [box_body([10., 20., 30.])];
    faber_render::save_view(&bodies, &path, None, SIZE, SIZE, view).unwrap();
    image::open(&path).unwrap().to_rgba8()
}

/// Returns the share of centre pixels that show the red part.
fn part_share(image: &RgbaImage) -> f64 {
    let (w, h) = image.dimensions();
    let mut hits = 0;
    let mut total = 0;
    for y in h / 2 - 20..h / 2 + 20 {
        for x in w / 2 - 20..w / 2 + 20 {
            total += 1;
            let p = image.get_pixel(x, y);
            if p[0] > 100 && p[0] > 2 * p[1] {
                hits += 1;
            }
        }
    }
    hits as f64 / total as f64
}

#[test]
fn top_view_shows_the_box() {
    let share = part_share(&render(View::Top));
    assert!(share >= 0.5, "top view part share {share}");
}

#[test]
fn bottom_view_shows_the_box() {
    let share = part_share(&render(View::Bottom));
    assert!(share >= 0.5, "bottom view part share {share}");
}
