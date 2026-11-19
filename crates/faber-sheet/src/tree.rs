//! Sample the wall outlines and retain the original edge indices.
use crate::{Edge, Wall};
use faber_geometry::sketch_loops::{rounded, sketch_arrays, slot};
use faber_model::{SketchLoop, SketchProfile, SketchSegment};
use glam::{DAffine2, DAffine3, DVec2};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::{simplify::SimplifyShape, single::SingleFloatOverlay},
};
use std::f64::consts::{PI, TAU};

type Point = [f64; 2];
const CHORD: f64 = 0.002;
const MAX_POINTS: usize = 65_536;
pub(crate) fn area(p: &[Point]) -> f64 {
    let o = DVec2::from(p[0]);
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(&a, &b)| (DVec2::from(a) - o).perp_dot(DVec2::from(b) - o))
        .sum::<f64>()
        * 0.5
}
fn valid(point: Point) -> bool {
    point.into_iter().all(|v| v.is_finite() && v.abs() <= 1e6)
}
fn steps(radius: f64, angle: f64) -> usize {
    // Keep a quadrant even when the radius is smaller than the chord tolerance.
    let span = (2. * (1. - (CHORD / radius).min(0.5)).acos()).min(PI / 16.);
    (angle.abs() / span).ceil().clamp(1., MAX_POINTS as f64) as usize
}
pub(crate) fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    (p - (a + d * ((p - a).dot(d) / d.length_squared()).clamp(0., 1.))).length()
}
struct Boundary {
    points: Vec<Point>,
    straight: Vec<Edge>,
    count: usize,
}
fn polygon(points: Vec<Point>) -> Boundary {
    let count = points.len();
    let straight = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .enumerate()
        .map(|(i, (&a, &b))| (i as u32 + 1, [a, b]))
        .collect();
    Boundary {
        points,
        straight,
        count,
    }
}
fn ellipse(centre: Point, rx: f64, ry: f64, rotation: f64) -> Result<Boundary, String> {
    if !valid(centre)
        || ![rx, ry].into_iter().all(|r| r.is_finite() && r > 0.)
        || !rotation.is_finite()
    {
        return Err("The ellipse needs finite radii greater than zero.".into());
    }
    let twist = DVec2::from_angle(rotation.to_radians());
    let n = steps(rx.max(ry), TAU).next_multiple_of(4);
    let points = (0..n)
        .map(|i| {
            let (s, c) = (TAU * i as f64 / n as f64).sin_cos();
            (DVec2::from(centre) + twist.rotate(DVec2::new(rx * c, ry * s))).to_array()
        })
        .collect();
    Ok(Boundary {
        points,
        straight: vec![],
        count: 1,
    })
}
pub(crate) fn arc(points: &mut Vec<Point>, a: DVec2, m: DVec2, b: DVec2) -> Result<(), String> {
    let d = m - a;
    let e = b - a;
    let cross = d.perp_dot(e);
    if cross.abs() < 1e-13 * d.length() * e.length() || cross == 0. {
        return Err("The three arc points must form a circle.".into());
    }
    let centre =
        a + (e.perp() * d.length_squared() - d.perp() * e.length_squared()) / (-2. * cross);
    let start = a - centre;
    let end = b - centre;
    let angle = start.perp_dot(end).atan2(start.dot(end));
    let angle = if cross > 0. {
        angle.rem_euclid(TAU)
    } else {
        -(-angle).rem_euclid(TAU)
    };
    let n = steps(start.length(), angle);
    if points.len() + n > MAX_POINTS {
        return Err("The sheet outline has too many curve samples.".into());
    }
    for i in 1..n {
        points.push(
            (centre + DVec2::from_angle(angle * i as f64 / n as f64).rotate(start)).to_array(),
        );
    }
    points.push(b.to_array());
    Ok(())
}
fn bezier(out: &mut Vec<Point>, p: [DVec2; 4], depth: usize) -> Result<(), String> {
    let [a, b, c, d] = p;
    if out.len() >= MAX_POINTS {
        return Err("The sheet outline has too many curve samples.".into());
    }
    if depth == 20 || (distance_to_segment(b, a, d).max(distance_to_segment(c, a, d)) <= CHORD) {
        out.push(d.to_array());
        return Ok(());
    }
    let ab = (a + b) * 0.5;
    let bc = (b + c) * 0.5;
    let cd = (c + d) * 0.5;
    let l = (ab + bc) * 0.5;
    let r = (bc + cd) * 0.5;
    let mid = (l + r) * 0.5;
    bezier(out, [a, ab, l, mid], depth + 1)?;
    bezier(out, [mid, r, cd, d], depth + 1)
}
fn boundary(source: &SketchLoop) -> Result<Boundary, String> {
    if let Some(lower) = rounded::lower(source)? {
        return boundary(&lower);
    }
    let part = match source {
        SketchLoop::Rectangle {
            origin: [x, y],
            size: [w, h],
        } if *w > 0. && *h > 0. => {
            polygon(vec![[*x, *y], [x + w, *y], [x + w, y + h], [*x, y + h]])
        }
        SketchLoop::Polygon { points, .. } => polygon(points.clone()),
        SketchLoop::RegularPolygon {
            centre,
            radius,
            sides,
            rotation,
            ..
        } => polygon(rounded::regular_points(
            *centre, *radius, *sides, *rotation,
        )?),
        SketchLoop::Circle { centre, radius } => ellipse(*centre, *radius, *radius, 0.)?,
        SketchLoop::Ellipse {
            centre,
            major_radius,
            minor_radius,
            rotation,
        } => ellipse(*centre, *major_radius, *minor_radius, *rotation)?,
        SketchLoop::Slot {
            centre,
            length,
            width,
            rotation,
        } => return boundary(&slot::boundary(*centre, *length, *width, *rotation)?),
        SketchLoop::Path { start, segments } => {
            if segments.len() > 4096 || !valid(*start) {
                return Err("Invalid sheet outline path.".into());
            }
            let mut points = vec![*start];
            let mut straight = Vec::new();
            for (i, s) in segments.iter().enumerate() {
                let a = *points.last().unwrap();
                match s {
                    SketchSegment::Line { end } => {
                        straight.push((i as u32 + 1, [a, *end]));
                        points.push(*end);
                    }
                    SketchSegment::Arc { via, end } if valid(*via) && valid(*end) => {
                        arc(&mut points, a.into(), (*via).into(), (*end).into())?;
                    }
                    SketchSegment::Bezier {
                        control1,
                        control2,
                        end,
                    } if [*control1, *control2, *end].into_iter().all(valid) => {
                        let cubic = [a, *control1, *control2, *end].map(DVec2::from);
                        bezier(&mut points, cubic, 0)?;
                    }
                    SketchSegment::Spline { .. } => {
                        return Err("The sheet outline cannot develop an interpolated spline. Use arcs or Bezier curves.".into());
                    }
                    _ => return Err("Invalid sheet curve coordinates.".into()),
                }
            }
            if DVec2::from(*points.last().unwrap()).distance((*start).into()) > 1e-7 {
                return Err("The sheet path must end at its start point.".into());
            }
            points.pop();
            Boundary {
                points,
                straight,
                count: segments.len(),
            }
        }
        _ => return Err("Use a closed sketch outline for the sheet wall.".into()),
    };
    let p = &part.points;
    if !(3..=MAX_POINTS).contains(&p.len())
        || p.iter().any(|&v| !valid(v))
        || p.iter()
            .zip(p.iter().cycle().skip(1))
            .any(|(&a, &b)| DVec2::from(a).distance(b.into()) < 1e-8)
        || area(p).abs() < 1e-12
    {
        return Err("The sheet outline needs distinct vertices and a closed area.".into());
    }
    let clean = p.simplify_shape(FillRule::EvenOdd);
    if clean.len() != 1
        || clean[0].len() != 1
        || (area(&clean[0][0]).abs() - area(p).abs()).abs() > 1e-6 * area(p).abs().max(1.)
    {
        return Err("The sheet outline crosses itself.".into());
    }
    Ok(part)
}

pub(crate) fn wall(
    id: &str,
    profile: SketchProfile,
    frame: DAffine3,
    flat_frame: DAffine2,
) -> Result<Wall, String> {
    let profile = sketch_arrays::expand(&profile)?;
    let outer = boundary(&profile.outer)?;
    let mut holes: Vec<Vec<Point>> = Vec::new();
    let mut count = outer.points.len();
    let mut bounds: Vec<(DVec2, DVec2)> = Vec::new();
    for h in &profile.holes {
        let hole = boundary(h)?.points;
        count += hole.len();
        if count > 262_144 {
            return Err(format!("Wall {id} has more than 262144 outline samples."));
        }
        let (lo, hi) = hole.iter().map(|&p| DVec2::from(p)).fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(a, b), p| (a.min(p), b.max(p)),
        );
        let out = hole.overlay(&outer.points, OverlayRule::Difference, FillRule::EvenOdd);
        if !out.is_empty() {
            return Err(format!("A hole extends outside wall {id}."));
        }
        let joined = outer
            .points
            .overlay(&hole, OverlayRule::Difference, FillRule::EvenOdd);
        if joined.len() != 1 || joined[0].len() != 2 {
            return Err(format!("A hole touches the edge of wall {id}."));
        }
        for (other, &(low, high)) in holes.iter().zip(&bounds) {
            if (hi.min(high) - lo.max(low)).min_element() < 0. {
                continue;
            }
            let merged = other.overlay(&hole, OverlayRule::Union, FillRule::NonZero);
            // Two disjoint contours have two shells and no common boundary vertex.
            if merged.len() != 2 || {
                let keys: std::collections::HashSet<_> =
                    merged[0][0].iter().map(|p| p.map(f64::to_bits)).collect();
                merged[1][0]
                    .iter()
                    .any(|p| keys.contains(&p.map(f64::to_bits)))
            } {
                return Err(format!("Two holes touch or overlap on wall {id}."));
            }
        }
        bounds.push((lo, hi));
        holes.push(hole);
    }
    Ok(Wall {
        ccw: area(&outer.points) > 0.,
        id: id.into(),
        profile,
        straight_edges: outer.straight,
        edge_count: outer.count,
        outline: outer.points,
        holes,
        frame,
        flat_frame,
        reliefs: vec![],
    })
}

pub(crate) fn charge_samples(count: &mut usize, extra: usize) -> Result<(), String> {
    let total = count.saturating_add(extra);
    if total > 1_000_000 {
        return Err(format!(
            "The sheet needs {total} outline samples. The limit is 1000000."
        ));
    }
    *count = total;
    Ok(())
}

pub(crate) fn wall_samples(w: &Wall) -> usize {
    w.outline.len() + w.holes.iter().map(Vec::len).sum::<usize>()
}

pub(crate) fn relief(wall: &mut Wall, cut: SketchLoop, count: &mut usize) -> Result<(), String> {
    let mut shape = vec![wall.outline.clone()];
    shape.extend_from_slice(&wall.holes);
    let boundary = boundary(&cut)?.points;
    let mut result = shape.overlay(&boundary, OverlayRule::Difference, FillRule::EvenOdd);
    if result.len() != 1 {
        return Err(format!("The relief cuts through wall {}.", wall.id));
    }
    let rings = result.pop().unwrap();
    *count -= wall_samples(wall);
    charge_samples(count, rings.iter().map(Vec::len).sum())?;
    let mut loops = rings.into_iter();
    wall.outline = loops.next().unwrap();
    wall.holes = loops.collect();
    wall.reliefs.push(cut);
    Ok(())
}

pub(crate) fn sample_loop(source: &SketchLoop) -> Result<Vec<[f64; 2]>, String> {
    Ok(boundary(source)?.points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_budget_counts_the_complete_sheet() {
        let mut count = 999_990;
        assert!(charge_samples(&mut count, 10).is_ok());
        let error = charge_samples(&mut count, 100).unwrap_err();
        assert!(
            error.contains("1000100") && error.contains("1000000"),
            "{error}"
        );
    }
}
