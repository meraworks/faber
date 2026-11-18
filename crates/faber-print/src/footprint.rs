//! Find a rotation in the bed plane that contains the projected mesh.
use glam::{DQuat, DVec2, DVec3};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// The outline of the projected part. Interior points cannot set its width.
fn hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    fn chain(points: impl Iterator<Item = DVec2>) -> Vec<DVec2> {
        let mut out: Vec<DVec2> = Vec::new();
        for p in points {
            while out.len() >= 2 {
                let end = out.len() - 1;
                if (out[end] - out[end - 1]).perp_dot(p - out[end]) > 0. {
                    break;
                }
                out.pop();
            }
            out.push(p);
        }
        out.pop();
        out
    }
    let mut result = chain(points.iter().copied());
    result.extend(chain(points.iter().rev().copied()));
    result
}

/// A hull vertex starts the edge with this outward normal.
struct Limit {
    angle: f64,
    point: DVec2,
}

fn extreme(limits: &[Limit], direction: f64) -> DVec2 {
    let angle = direction.rem_euclid(TAU);
    let i = limits.partition_point(|v| v.angle < angle);
    limits[i % limits.len()].point
}

/// Between two successive edge normals, four fixed vertices set both extents.
/// Each extent is a positive sinusoid, concave on that interval. The smaller
/// maximum fraction of bed use occurs at an endpoint or where the fractions meet.
pub(crate) fn turn_to_fit(
    vertices: &[[f64; 3]],
    rotation: DQuat,
    size: [f64; 3],
    bed: [f64; 3],
    eps: f64,
) -> Option<(DQuat, [f64; 3])> {
    if size[2] > bed[2] + eps || size[0].max(size[1]) > bed[0].hypot(bed[1]) + eps {
        return None;
    }
    let origin = DVec3::from(vertices[0]);
    let points = vertices
        .iter()
        .map(|&v| (rotation * (DVec3::from(v) - origin)).truncate());
    let outline = hull(points.collect());
    if outline.len() < 2 {
        return None;
    }
    let mut limits = Vec::with_capacity(outline.len());
    let mut breaks = vec![0., PI];
    for (i, &p) in outline.iter().enumerate() {
        let edge = outline[(i + 1) % outline.len()] - p;
        let angle = (-edge.x).atan2(edge.y).rem_euclid(TAU);
        limits.push(Limit { angle, point: p });
        breaks.push((-angle).rem_euclid(PI));
        breaks.push((FRAC_PI_2 - angle).rem_euclid(PI));
    }
    limits.sort_unstable_by(|a, b| a.angle.total_cmp(&b.angle));
    breaks.sort_unstable_by(f64::total_cmp);
    breaks.dedup();
    for interval in breaks.windows(2) {
        let (a, b) = (interval[0], interval[1]);
        let mid = (a + b) * 0.5;
        let dx = extreme(&limits, -mid) - extreme(&limits, PI - mid);
        let dy = extreme(&limits, FRAC_PI_2 - mid) - extreme(&limits, -FRAC_PI_2 - mid);
        // Width = dx.x cos(t) - dx.y sin(t). Depth = dy.x sin(t) + dy.y cos(t).
        let cos_factor = dx.x / bed[0] - dy.y / bed[1];
        let sin_factor = -dx.y / bed[0] - dy.x / bed[1];
        let balance = (-cos_factor).atan2(sin_factor).rem_euclid(PI);
        for t in [a, b, balance] {
            if !(a..=b).contains(&t) {
                continue;
            }
            let (sin, cos) = t.sin_cos();
            let width = dx.dot(DVec2::new(cos, -sin));
            let depth = dy.dot(DVec2::new(sin, cos));
            if width <= bed[0] + eps && depth <= bed[1] + eps {
                return Some((
                    DQuat::from_rotation_z(t) * rotation,
                    [width, depth, size[2]],
                ));
            }
        }
    }
    None
}
