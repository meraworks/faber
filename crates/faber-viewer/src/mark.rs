//! The Faber mark: an F solid, extruded and seen in isometric view.
//!
//! The geometry is on the 64-unit grid of `assets/icon.svg`. The F lies in a
//! plane that faces the viewer. Letter points are (depth, h, v) in letter
//! units: h runs right, v runs up, and depth runs from the back (0) to the
//! front face (`DEPTH`).

use std::f32::consts::TAU;

use eframe::egui::{self, Color32, Pos2, Shape, Stroke, Vec2};

/// Light tone of the top faces.
pub const TOP: Color32 = Color32::from_rgb(0x9A, 0xDE, 0xD6);
/// Front face of the F. This is the accent colour.
pub const LEFT: Color32 = Color32::from_rgb(0x64, 0xCD, 0xC0);
/// Dark tone of the side faces.
pub const RIGHT: Color32 = Color32::from_rgb(0x3C, 0x7B, 0x73);
/// Background of the icon square.
const BACK: Color32 = Color32::from_rgb(0x10, 0x13, 0x18);

/// Extrusion depth in letter units.
const DEPTH: f32 = 5.0;
const COS30: f32 = 0.866_025_4;
/// Grid units per letter unit. The mark is 49.5 units high.
const SCALE: f32 = 49.5 / 57.5;
/// Projected centre of the raw bounding box, x and y.
const RAW_CENTRE: (f32, f32) = (COS30 * 35.0 / 2.0, -26.25);

/// Front face pieces. Each piece is convex, with corners in order.
const FRONT_PIECES: [&[(f32, f32)]; 3] = [
    &[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 40.0),
        (8.0, 40.0),
        (0.0, 32.0),
    ],
    &[(10.0, 12.0), (24.0, 12.0), (24.0, 22.0), (10.0, 22.0)],
    &[(10.0, 30.0), (30.0, 30.0), (30.0, 40.0), (10.0, 40.0)],
];
/// Visible edges with light top faces.
const TOP_EDGES: [((f32, f32), (f32, f32)); 3] = [
    ((30.0, 40.0), (8.0, 40.0)),
    ((8.0, 40.0), (0.0, 32.0)),
    ((24.0, 22.0), (10.0, 22.0)),
];
/// Visible edges with dark side faces.
const SIDE_EDGES: [((f32, f32), (f32, f32)); 1] = [((0.0, 32.0), (0.0, 0.0))];
/// The drilled hole through the top arm, on the front face.
const HOLE_CENTRE: (f32, f32) = (20.0, 35.0);
const HOLE_RADIUS: f32 = 2.8;
const HOLE_POINTS: usize = 48;

/// Returns the grid point of a letter point.
fn grid(depth: f32, h: f32, v: f32) -> (f32, f32) {
    let rx = COS30 * (depth + h);
    let ry = 0.5 * (depth - h) - v;
    (
        32.0 + SCALE * (rx - RAW_CENTRE.0),
        32.0 + SCALE * (ry - RAW_CENTRE.1),
    )
}

/// Returns the visible faces in painter order, with their corners.
/// The front pieces come last, so they cover the sides.
fn faces() -> Vec<(Color32, Vec<(f32, f32)>)> {
    let side = |(a, b): ((f32, f32), (f32, f32))| {
        vec![
            grid(DEPTH, a.0, a.1),
            grid(DEPTH, b.0, b.1),
            grid(0.0, b.0, b.1),
            grid(0.0, a.0, a.1),
        ]
    };
    let mut out: Vec<_> = TOP_EDGES.iter().map(|&e| (TOP, side(e))).collect();
    out.extend(SIDE_EDGES.iter().map(|&e| (RIGHT, side(e))));
    out.extend(FRONT_PIECES.iter().map(|piece| {
        let pts = piece.iter().map(|&(h, v)| grid(DEPTH, h, v)).collect();
        (LEFT, pts)
    }));
    out
}

/// Returns the hole on the front face as a convex polygon.
fn hole() -> Vec<(f32, f32)> {
    (0..HOLE_POINTS)
        .map(|i| {
            let t = TAU * i as f32 / HOLE_POINTS as f32;
            grid(
                DEPTH,
                HOLE_CENTRE.0 + HOLE_RADIUS * t.cos(),
                HOLE_CENTRE.1 + HOLE_RADIUS * t.sin(),
            )
        })
        .collect()
}

/// Draws the mark. `centre` is the mark centre and `scale` is pixels per grid unit.
/// `hole_colour` must match the colour behind the mark.
pub fn paint(painter: &egui::Painter, centre: Pos2, scale: f32, hole_colour: Color32) {
    let at = |(x, y): (f32, f32)| centre + Vec2::new(x - 32.0, y - 32.0) * scale;
    for (colour, corners) in faces() {
        let points = corners.iter().map(|&p| at(p)).collect();
        painter.add(Shape::convex_polygon(points, colour, Stroke::NONE));
    }
    let points = hole().iter().map(|&p| at(p)).collect();
    painter.add(Shape::convex_polygon(points, hole_colour, Stroke::NONE));
}

/// Returns the mark on a dark rounded square, for the window icon. `size` is in pixels.
pub fn icon(size: u32) -> egui::IconData {
    const SUB: u32 = 4;
    let faces = faces();
    let hole = hole();
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut rgb = [0f32; 3];
            let mut seen = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let to_grid = 64.0 / size as f32;
                    let fx = (px as f32 + (sx as f32 + 0.5) / SUB as f32) * to_grid;
                    let fy = (py as f32 + (sy as f32 + 0.5) / SUB as f32) * to_grid;
                    if let Some(c) = sample((fx, fy), &faces, &hole) {
                        for (acc, v) in rgb.iter_mut().zip([c.r(), c.g(), c.b()]) {
                            *acc += f32::from(v);
                        }
                        seen += 1;
                    }
                }
            }
            let total = (SUB * SUB) as f32;
            let colour = if seen == 0 {
                [0.0; 3]
            } else {
                rgb.map(|v| v / seen as f32)
            };
            rgba.extend(colour.map(|v| v.round() as u8));
            rgba.push((seen as f32 / total * 255.0).round() as u8);
        }
    }
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}

/// Returns the colour at a grid point, or None outside the rounded square.
/// The test order matches the paint order: the hole, then the faces from the front.
fn sample(
    (x, y): (f32, f32),
    faces: &[(Color32, Vec<(f32, f32)>)],
    hole: &[(f32, f32)],
) -> Option<Color32> {
    // The square spans 0.5 to 63.5 with corner radius 14.
    let (qx, qy) = (x.clamp(14.5, 49.5), y.clamp(14.5, 49.5));
    if (x - qx).powi(2) + (y - qy).powi(2) > 14.0 * 14.0 {
        return None;
    }
    if convex_contains((x, y), hole) {
        return Some(BACK);
    }
    for (colour, corners) in faces.iter().rev() {
        if convex_contains((x, y), corners) {
            return Some(*colour);
        }
    }
    Some(BACK)
}

/// Returns true if `p` lies inside a convex polygon with corners in order.
fn convex_contains(p: (f32, f32), corners: &[(f32, f32)]) -> bool {
    let n = corners.len();
    let cross = (0..n).map(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % n]);
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    });
    let (mut pos, mut neg) = (true, true);
    for c in cross {
        pos &= c >= 0.0;
        neg &= c <= 0.0;
    }
    pos || neg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_transparent_in_the_corner_and_shows_the_mark() {
        let icon = icon(64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        let pixel = |x: usize, y: usize| &icon.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(pixel(0, 0)[3], 0);
        // The middle of the stem is on the front face.
        let (fx, fy) = grid(DEPTH, 5.0, 20.0);
        assert_eq!(&pixel(fx as usize, fy as usize)[..3], &[0x64, 0xCD, 0xC0]);
    }

    #[test]
    fn faces_and_hole_follow_the_paint_order() {
        let faces = faces();
        let hole = hole();
        let colour = |p: (f32, f32)| sample(p, &faces, &hole);
        // The front of the stem.
        assert_eq!(colour(grid(DEPTH, 5.0, 20.0)), Some(LEFT));
        // The drilled hole shows the background.
        let (hc, vc) = HOLE_CENTRE;
        assert_eq!(colour(grid(DEPTH, hc, vc)), Some(BACK));
        // The top of the top arm, seen between the front and the back.
        assert_eq!(colour(grid(2.5, 20.0, 40.0)), Some(TOP));
        // The left side of the stem, outside the front face.
        assert_eq!(colour(grid(2.5, 0.0, 20.0)), Some(RIGHT));
    }
}
