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
            normals: Vec::new(),
            vertices,
            triangles,
        },
        volume: size.iter().product(),
        bounds: [[-x, -y, -z], [x, y, z]],
        face_of_triangle: Vec::new(),
        edge_lines: Default::default(),
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
            if p[0] > 100 && u16::from(p[0]) > 2 * u16::from(p[1]) {
                hits += 1;
            }
        }
    }
    hits as f64 / total as f64
}

#[test]
fn visibility_changes_reuse_the_uploaded_geometry() {
    let gpu = faber_render::Gpu::headless().unwrap();
    let bodies = [box_body([10., 20., 30.])];
    let list = gpu
        .upload(&bodies, &Default::default(), None, None)
        .unwrap();
    let target = gpu.target(SIZE, SIZE);
    let mut camera = faber_scene::Camera::fit(&bodies);
    camera.set_view(View::Top);
    let mut hidden = std::collections::BTreeSet::new();
    for show in [true, false, true] {
        if show {
            hidden.clear();
        } else {
            hidden.insert("box".to_string());
        }
        gpu.draw_visible_view(
            &target,
            &list,
            camera,
            None,
            None,
            None,
            &Default::default(),
            &hidden,
        );
        let frame = RgbaImage::from_raw(SIZE, SIZE, gpu.read(&target).unwrap()).unwrap();
        assert_eq!(part_share(&frame) > 0.5, show);
    }
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

#[test]
fn scene_background_is_light_and_bottom_face_is_lit() {
    let image = render(View::Bottom);
    let background = image.get_pixel(2, 2);
    assert!(background[0] >= 200 && background[1] >= 200 && background[2] >= 200);
    let bottom = image.get_pixel(SIZE / 2, SIZE / 2);
    assert!(bottom[0] >= 185, "Bottom face is too dark: {bottom:?}");
}
