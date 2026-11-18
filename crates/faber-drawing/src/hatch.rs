use crate::path;
use faber_ports::{Result, vector::Path2};
use glam::DVec2;

/// In the rotated basis, each hatch line has a constant y coordinate.
pub(crate) fn lines(rings: &[Vec<[f64; 2]>], spacing: f64) -> Result<Vec<Path2>> {
    let x = DVec2::splat(std::f64::consts::FRAC_1_SQRT_2);
    let y = x.perp();
    let mut edges = Vec::new();
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for ring in rings.iter().filter(|r| r.len() >= 3) {
        for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
            let rot = |p: [f64; 2]| {
                let v = DVec2::from(p);
                DVec2::new(v.dot(x), v.dot(y))
            };
            let (a, b) = (rot(a), rot(b));
            if !a.is_finite() || !b.is_finite() {
                return Err("The section loop is invalid.".into());
            }
            if (b.y - a.y).abs() > 1e-12 {
                min = min.min(a.y.min(b.y));
                max = max.max(a.y.max(b.y));
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return Ok(vec![]);
    }
    let start = (min / spacing).ceil();
    let rows = (max / spacing).ceil() - start;
    let checks = rows * edges.len() as f64;
    if !checks.is_finite() || checks > 50_000_000. {
        return Err(format!(
            "The section hatch needs {checks:.0} edge checks. The limit is 50000000."
        ));
    }
    let mut out = Vec::new();
    let mut intersections = Vec::new();
    for i in 0..rows as usize {
        let row = (start + i as f64) * spacing;
        intersections.clear();
        for &(a, b) in &edges {
            if (a.y <= row && row < b.y) || (b.y <= row && row < a.y) {
                intersections.push(a.x + (b.x - a.x) * ((row - a.y) / (b.y - a.y)));
            }
        }
        intersections.sort_by(f64::total_cmp);
        for &[a, b] in intersections.as_chunks::<2>().0 {
            if b > a + 1e-10 {
                out.push(path([x * a + y * row, x * b + y * row], false));
            }
        }
    }
    Ok(out)
}
