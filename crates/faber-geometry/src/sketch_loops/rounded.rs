//! Rounded corners for sketch loops. Each corner is an exact arc tangent to both edges.
use faber_model::{SketchLoop, SketchSegment};
use faber_ports::Result;
use glam::DVec2;
use std::f64::consts::{PI, TAU};

/// Lowers a rounded loop to lines and arcs. Other loops return `None`.
pub fn lower(boundary: &SketchLoop) -> Result<Option<SketchLoop>> {
    match boundary {
        SketchLoop::RoundedRectangle {
            centre,
            width,
            height,
            radius,
            rotation,
        } => {
            super::positive(*width)?;
            super::positive(*height)?;
            super::positive(*radius)?;
            let (sin, cos) = rotation.to_radians().sin_cos();
            let (half_width, half_height) = (width / 2., height / 2.);
            let points = [
                (-half_width, -half_height),
                (half_width, -half_height),
                (half_width, half_height),
                (-half_width, half_height),
            ]
            .map(|(x, y)| [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos]);
            polygon(&points, *radius, "rounded_rectangle").map(Some)
        }
        SketchLoop::RegularPolygon {
            centre,
            radius,
            sides,
            rotation,
            corner_radius,
        } => {
            let Some(corner) = corner(*corner_radius)? else {
                return Ok(None);
            };
            let points = regular_points(*centre, *radius, *sides, *rotation)?;
            polygon(&points, corner, "regular_polygon").map(Some)
        }
        SketchLoop::Polygon {
            points,
            corner_radius,
        } => {
            let Some(corner) = corner(*corner_radius)? else {
                return Ok(None);
            };
            polygon(points, corner, "polygon").map(Some)
        }
        _ => Ok(None),
    }
}

/// Vertices of a regular polygon. The first vertex is at the rotation angle.
pub fn regular_points(
    centre: [f64; 2],
    radius: f64,
    sides: usize,
    rotation: f64,
) -> Result<Vec<[f64; 2]>> {
    super::positive(radius)?;
    if !(3..=4096).contains(&sides) {
        return Err("Regular polygon needs between 3 and 4096 sides".into());
    }
    Ok((0..sides)
        .map(|i| {
            let angle = rotation.to_radians() + TAU * i as f64 / sides as f64;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect())
}

/// Corner radius from the model. Zero or absent means sharp.
fn corner(radius: Option<f64>) -> Result<Option<f64>> {
    match radius {
        None => Ok(None),
        Some(value) if value.is_finite() && (0.0..=1e6).contains(&value) => {
            Ok((value > 0.).then_some(value))
        }
        Some(_) => Err("Corner radius must be between 0 and 1000000 mm".into()),
    }
}

struct Corner {
    to_prev: DVec2,
    to_next: DVec2,
    half: f64,
}

/// Rounds every corner of a closed polygon with one radius.
fn polygon(points: &[[f64; 2]], radius: f64, name: &str) -> Result<SketchLoop> {
    let n = points.len();
    if n < 3 {
        return Err(format!("{name} loop needs at least 3 points"));
    }
    if points
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("Sketch coordinates are outside the supported range".into());
    }
    let points: Vec<DVec2> = points.iter().map(|p| DVec2::from(*p)).collect();
    let mut corners = Vec::with_capacity(n);
    for i in 0..n {
        let before = points[(i + n - 1) % n] - points[i];
        let after = points[(i + 1) % n] - points[i];
        if before.length() < 1e-9 || after.length() < 1e-9 {
            return Err(format!("{name} loop has a repeated point"));
        }
        let to_prev = before.normalize();
        let to_next = after.normalize();
        let angle = to_prev.dot(to_next).clamp(-1., 1.).acos();
        if !(1e-6..PI - 1e-6).contains(&angle) {
            return Err(format!(
                "{name} loop has a straight or folded corner. Remove that point or the corner radius."
            ));
        }
        corners.push(Corner {
            to_prev,
            to_next,
            half: angle / 2.,
        });
    }
    // The tangent distance on each side is radius / tan(half angle).
    let cot = |corner: &Corner| 1. / corner.half.tan();
    let mut limit = f64::INFINITY;
    let mut fits = true;
    for i in 0..n {
        let j = (i + 1) % n;
        let length = (points[j] - points[i]).length();
        let sum = cot(&corners[i]) + cot(&corners[j]);
        limit = limit.min(length / sum);
        fits &= radius * sum <= length - 1e-6;
    }
    if !fits {
        return Err(format!(
            "{name} loop: corner radius {radius:.3} mm is too large. The limit is {limit:.3} mm."
        ));
    }
    let tangent = |i: usize| radius * cot(&corners[i]);
    let start = points[0] + corners[0].to_next * tangent(0);
    let mut segments = Vec::with_capacity(2 * n);
    // Walk the corners 1, 2, ..., n - 1, 0. Each line ends at an entry tangent point, then an arc leaves.
    for i in (1..=n).map(|k| k % n) {
        let corner = &corners[i];
        let t = tangent(i);
        let entry = points[i] + corner.to_prev * t;
        let exit = points[i] + corner.to_next * t;
        // The arc centre lies on the bisector of the corner angle, convex or reflex.
        let bisector = (corner.to_prev + corner.to_next).normalize();
        let reach = radius / corner.half.sin() - radius;
        let via = points[i] + bisector * reach;
        segments.push(SketchSegment::Line {
            end: entry.to_array(),
        });
        segments.push(SketchSegment::Arc {
            via: via.to_array(),
            end: exit.to_array(),
        });
    }
    Ok(SketchLoop::Path {
        start: start.to_array(),
        segments,
    })
}
