//! Screen distance to the visible portion of a placed edge.

use super::Section;
use faber_ports::EdgeLines;
use glam::{DMat4, DVec2, DVec3, DVec4, Mat4};

#[derive(Clone, Copy, Debug)]
pub struct EdgeHit {
    pub edge: u32,
    pub distance_px: f32,
    pub depth: f32,
    pub point: [f64; 3],
}

pub fn pick_edge(
    lines: &EdgeLines,
    transform: DMat4,
    view: Mat4,
    rect: [[f32; 2]; 2],
    cursor: [f32; 2],
    max_px: f32,
) -> Option<u32> {
    pick_edge_hit(lines, transform, view, rect, cursor, max_px, None).map(|hit| hit.edge)
}

pub fn pick_edge_hit(
    lines: &EdgeLines,
    transform: DMat4,
    view: Mat4,
    rect: [[f32; 2]; 2],
    cursor: [f32; 2],
    max_px: f32,
    cut: Option<Section>,
) -> Option<EdgeHit> {
    pick_edge_hit_where(lines, transform, view, rect, cursor, max_px, cut, |_| true)
}

/// Pick the closest segment that passes the visibility test.
#[allow(clippy::too_many_arguments)]
pub fn pick_edge_hit_where(
    lines: &EdgeLines,
    transform: DMat4,
    view: Mat4,
    rect: [[f32; 2]; 2],
    cursor: [f32; 2],
    max_px: f32,
    cut: Option<Section>,
    mut visible: impl FnMut(&EdgeHit) -> bool,
) -> Option<EdgeHit> {
    let [min, max] = rect.map(|p| DVec2::from(p.map(f64::from)));
    let size = max - min;
    let cursor = DVec2::from(cursor.map(f64::from));
    if !size.cmpgt(DVec2::ZERO).all()
        || !size.is_finite()
        || !max_px.is_finite()
        || max_px < 0.
        || !cursor.cmpge(min).all()
        || !cursor.cmple(max).all()
    {
        return None;
    }
    let matrix = view.as_dmat4();
    let plane = cut.map(|s| {
        let normal = DVec3::from(s.normal).normalize();
        let sign = if s.reverse { -1. } else { 1. };
        (-normal * sign).extend(s.offset * sign)
    });
    let screen = |clip: DVec4| {
        let ndc = clip.truncate() / clip.w;
        min + (DVec2::new(ndc.x, -ndc.y) + DVec2::ONE) * 0.5 * size
    };
    let mut best: Option<EdgeHit> = None;
    for (index, range) in lines.ranges.iter().enumerate() {
        let Some(points) = lines.points.get(range.start as usize..range.end as usize) else {
            continue;
        };
        for pair in points.windows(2) {
            let a = transform.transform_point3(DVec3::from(pair[0]));
            let b = transform.transform_point3(DVec3::from(pair[1]));
            let ca = matrix * a.extend(1.);
            let cb = matrix * b.extend(1.);
            let Some([start, end]) = clip_segment(
                ca,
                cb,
                plane.map(|p| [p.dot(a.extend(1.)), p.dot(b.extend(1.))]),
            ) else {
                continue;
            };
            let left = ca.lerp(cb, start);
            let right = ca.lerp(cb, end);
            if left.w <= 0. || right.w <= 0. {
                continue;
            }
            let sa = screen(left);
            let span = screen(right) - sa;
            let length_squared = span.length_squared();
            let za = left.z / left.w;
            let zb = right.z / right.w;
            let t = if length_squared > 0. {
                ((cursor - sa).dot(span) / length_squared).clamp(0., 1.)
            } else if za <= zb {
                0.
            } else {
                1.
            };
            let distance_px = cursor.distance(sa + t * span) as f32;
            let depth = ((1. - t) * za + t * zb) as f32;
            if distance_px > max_px
                || !distance_px.is_finite()
                || best.is_some_and(|h| {
                    h.distance_px < distance_px
                        || (h.distance_px == distance_px && h.depth <= depth)
                })
            {
                continue;
            }
            // The world parameter must include perspective division.
            let u = t * left.w / ((1. - t) * right.w + t * left.w);
            let world = a.lerp(b, start + (end - start) * u);
            let hit = EdgeHit {
                edge: index as u32 + 1,
                distance_px,
                depth,
                point: world.to_array(),
            };
            if visible(&hit) {
                best = Some(hit);
            }
        }
    }
    best
}

fn clip_segment(a: DVec4, b: DVec4, cut: Option<[f64; 2]>) -> Option<[f64; 2]> {
    let mut start = 0.0f64;
    let mut end = 1.0f64;
    let planes = [
        [a.w + a.x, b.w + b.x],
        [a.w - a.x, b.w - b.x],
        [a.w + a.y, b.w + b.y],
        [a.w - a.y, b.w - b.y],
        [a.z, b.z],
        [a.w - a.z, b.w - b.z],
    ];
    for [first, last] in planes.into_iter().chain(cut) {
        if !first.is_finite() || !last.is_finite() || (first < 0. && last < 0.) {
            return None;
        }
        if first < 0. {
            start = start.max(first / (first - last));
        }
        if last < 0. {
            end = end.min(first / (first - last));
        }
        if start > end {
            return None;
        }
    }
    Some([start, end])
}
