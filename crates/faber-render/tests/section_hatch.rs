use faber_ports::{Body, Mesh};
use faber_scene::{Section, View};
use image::RgbaImage;
use std::path::PathBuf;

const SIZE: u32 = 320;
const COLOUR: [u8; 3] = [240, 60, 60];

/// Adds the four side walls of a rectangle in Y and Z, running along X from -10 to 10.
fn side_walls(
    vertices: &mut Vec<[f64; 3]>,
    triangles: &mut Vec<[u32; 3]>,
    y: [f64; 2],
    z: [f64; 2],
) {
    let ring = [[y[0], z[0]], [y[1], z[0]], [y[1], z[1]], [y[0], z[1]]];
    for k in 0..4 {
        let (a, b) = (ring[k], ring[(k + 1) % 4]);
        let start = vertices.len() as u32;
        vertices.extend([
            [-10., a[0], a[1]],
            [10., a[0], a[1]],
            [10., b[0], b[1]],
            [-10., b[0], b[1]],
        ]);
        triangles.push([start, start + 1, start + 2]);
        triangles.push([start, start + 2, start + 3]);
    }
}

/// Builds a square tube along X. The section at x = 0 cuts a rectangular hole.
fn tube_body() -> Body {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    side_walls(&mut vertices, &mut triangles, [-20., 20.], [-30., 30.]);
    side_walls(&mut vertices, &mut triangles, [-10., 10.], [-15., 15.]);
    Body {
        id: "tube".into(),
        name: "Tube".into(),
        group: String::new(),
        colour: COLOUR,
        mesh: Mesh {
            normals: Vec::new(),
            vertices,
            triangles,
        },
        volume: 20. * 40. * 60. - 20. * 20. * 30.,
        bounds: [[-10., -20., -30.], [10., 20., 30.]],
        face_of_triangle: Vec::new(),
        shape: None,
    }
}

fn render() -> RgbaImage {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("section-hatch.png");
    let plane = Section {
        normal: [1., 0., 0.],
        offset: 0.,
        reverse: false,
    };
    faber_render::save_view(&[tube_body()], &path, Some(plane), SIZE, SIZE, View::Right).unwrap();
    image::open(&path).unwrap().to_rgba8()
}

/// Returns true for a cut face pixel. Cut faces use the body red.
fn is_cut(p: [u8; 4]) -> bool {
    let [r, g, b] = [p[0], p[1], p[2]].map(u16::from);
    r > 80 && r > 2 * g && r > 2 * b
}

/// Returns true for the light hatch line, not the dark gap.
fn is_line(p: [u8; 4]) -> bool {
    p[0] > 190
}

#[test]
fn cut_face_has_a_hatch_of_two_colours() {
    let image = render();
    let (x0, x1) = (SIZE / 2 - 30, SIZE / 2 + 30);
    let (y0, y1) = (SIZE / 2 + 60, SIZE / 2 + 95);
    let mut lines = 0;
    let mut gaps = 0;
    let mut cut = 0;
    let mut same_diagonal = 0;
    let mut compared = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = image.get_pixel(x, y).0;
            if is_cut(p) {
                cut += 1;
            }
            if is_cut(p) && is_line(p) {
                lines += 1;
            } else if is_cut(p) {
                gaps += 1;
            }
            // A 45 degree hatch repeats along the diagonal (x + 1, y - 1).
            let q = image.get_pixel(x + 1, y - 1).0;
            if is_cut(p) && is_cut(q) {
                compared += 1;
                if is_line(p) == is_line(q) {
                    same_diagonal += 1;
                }
            }
        }
    }
    let total = ((x1 - x0) * (y1 - y0)) as usize;
    assert!(
        cut as usize > total * 9 / 10,
        "cut face covers {cut} of {total}"
    );
    assert!(lines as usize > total / 5, "hatch lines {lines}");
    assert!(
        (total / 20..total / 5).contains(&(gaps as usize)),
        "Thin hatch pixels {gaps} of {total}"
    );
    assert!(
        same_diagonal * 100 >= compared * 90,
        "hatch is not at 45 degrees: {same_diagonal} of {compared}"
    );
}

#[test]
fn cut_face_hatch_has_constant_pixel_period() {
    let image = render();
    let x = SIZE / 2;
    let mut transitions = 0;
    let mut previous = None;
    for y in SIZE / 2 + 60..SIZE / 2 + 95 {
        let p = image.get_pixel(x, y).0;
        let line = is_cut(p) && is_line(p);
        if let Some(before) = previous
            && before != line
        {
            transitions += 1;
        }
        previous = Some(line);
    }
    // Lines 9 pixels apart at 45 degrees give a column period of about 12.7 pixels,
    // so 35 pixels hold about 5 changes.
    assert!(
        (4..=7).contains(&transitions),
        "hatch changes {transitions} times in 35 pixels"
    );
}

#[test]
fn hole_in_the_cut_face_stays_open() {
    let image = render();
    let centre = image.get_pixel(SIZE / 2, SIZE / 2).0;
    assert!(!is_cut(centre), "hole centre shows a cut face: {centre:?}");
    let mut cut_in_hole = 0;
    for y in SIZE / 2 - 20..SIZE / 2 + 20 {
        for x in SIZE / 2 - 20..SIZE / 2 + 20 {
            if is_cut(image.get_pixel(x, y).0) {
                cut_in_hole += 1;
            }
        }
    }
    assert_eq!(cut_in_hole, 0, "cut face covers the hole");
}
