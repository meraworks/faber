use crate::path;
use faber_ports::vector::Path2;
use glam::DVec2;
use std::f64::consts::TAU;

fn segment(a: DVec2, b: DVec2, c: DVec2, r: f64) -> Option<Path2> {
    let d = b - a;
    let len = d.length_squared();
    if len <= 1e-24 {
        return None;
    }
    let t = (c - a).dot(d) / len;
    let dist = (a + d * t - c).length_squared();
    if dist > r * r {
        return None;
    }
    let span = ((r * r - dist) / len).sqrt();
    let start = (t - span).max(0.);
    let end = (t + span).min(1.);
    (end > start + 1e-12).then(|| path([a + d * start, a + d * end], false))
}
/// Split at exact circle intersections. Keep the intervals inside the detail.
pub(crate) fn inside(p: &Path2, c: DVec2, r: f64) -> Vec<Path2> {
    let (centre, radius, start, sweep) = match p {
        Path2::Polyline { points, closed } => {
            let mut out = Vec::new();
            for ab in points.windows(2) {
                out.extend(segment(ab[0].into(), ab[1].into(), c, r));
            }
            if *closed && points.len() > 2 {
                out.extend(segment(
                    points.last().copied().unwrap().into(),
                    points[0].into(),
                    c,
                    r,
                ));
            }
            return out;
        }
        Path2::Circle { centre, radius } => (DVec2::from(*centre), *radius, 0., TAU),
        Path2::Arc {
            centre,
            radius,
            start_deg,
            end_deg,
        } => (
            DVec2::from(*centre),
            *radius,
            start_deg.to_radians(),
            (end_deg - start_deg).rem_euclid(360.).to_radians(),
        ),
    };
    let delta = c - centre;
    let dist = delta.length();
    if dist + radius <= r + 1e-10 {
        return vec![p.clone()];
    }
    if dist >= r + radius || radius >= r + dist || dist <= 1e-12 {
        return vec![];
    }
    let half = ((radius * radius + dist * dist - r * r) / (2. * radius * dist))
        .clamp(-1., 1.)
        .acos();
    let mid = delta.y.atan2(delta.x);
    let mut stops = vec![0., sweep];
    for a in [mid - half, mid + half] {
        let t = (a - start).rem_euclid(TAU);
        if t > 1e-12 && t < sweep - 1e-12 {
            stops.push(t);
        }
    }
    stops.sort_by(f64::total_cmp);
    stops
        .windows(2)
        .filter_map(|s| {
            let theta = start + (s[0] + s[1]) / 2.;
            let sample = centre + DVec2::from_angle(theta) * radius;
            (sample.distance_squared(c) <= r * r).then(|| Path2::Arc {
                centre: centre.into(),
                radius,
                start_deg: (start + s[0]).to_degrees(),
                end_deg: (start + s[1]).to_degrees(),
            })
        })
        .collect()
}
