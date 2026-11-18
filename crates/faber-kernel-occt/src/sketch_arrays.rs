use faber_model::{SketchLoop, SketchProfile, SketchSegment};
use faber_ports::Result;

pub(crate) fn expand(profile: &SketchProfile) -> Result<SketchProfile> {
    if matches!(profile.outer, SketchLoop::Grid { .. }) {
        return Err("A sketch outer boundary cannot be an array".into());
    }
    let mut holes = Vec::new();
    for hole in &profile.holes {
        append(hole, [0.; 2], 0, &mut holes)?;
    }
    Ok(SketchProfile {
        outer: profile.outer.clone(),
        holes,
    })
}

fn append(
    item: &SketchLoop,
    offset: [f64; 2],
    depth: usize,
    output: &mut Vec<SketchLoop>,
) -> Result<()> {
    if depth > 16 {
        return Err("Sketch arrays exceed 16 nested levels".into());
    }
    if matches!(item, SketchLoop::Text { .. }) {
        return Err("Text can only be the outer boundary of a sketch".into());
    }
    if let SketchLoop::Grid { item, count, step } = item {
        if count.contains(&0) || count[0].checked_mul(count[1]).is_none_or(|n| n > 256) {
            return Err("Sketch grid needs between 1 and 256 copies".into());
        }
        if step
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Sketch grid steps are outside the supported range".into());
        }
        for axis in 0..2 {
            if count[axis] > 1 && step[axis][0].hypot(step[axis][1]) < 1e-7 {
                return Err("A repeated grid direction needs a non-zero step".into());
            }
        }
        for row in 0..count[1] {
            for column in 0..count[0] {
                let shift = std::array::from_fn(|axis| {
                    offset[axis] + column as f64 * step[0][axis] + row as f64 * step[1][axis]
                });
                append(item, shift, depth + 1, output)?;
            }
        }
        return Ok(());
    }
    if output.len() >= 256 {
        return Err("Sketch has more than 256 holes".into());
    }
    let mut item = item.clone();
    let shift = |p: &mut [f64; 2]| {
        p[0] += offset[0];
        p[1] += offset[1];
    };
    match &mut item {
        SketchLoop::Slot { centre, .. }
        | SketchLoop::RoundedRectangle { centre, .. }
        | SketchLoop::Ellipse { centre, .. }
        | SketchLoop::RegularPolygon { centre, .. }
        | SketchLoop::Circle { centre, .. } => shift(centre),
        SketchLoop::Rectangle { origin, .. } => shift(origin),
        SketchLoop::Polygon { points, .. } => points.iter_mut().for_each(shift),
        SketchLoop::Path { start, segments } => {
            shift(start);
            for segment in segments {
                match segment {
                    SketchSegment::Spline { points, .. } => points.iter_mut().for_each(shift),
                    SketchSegment::Line { end } => shift(end),
                    SketchSegment::Arc { via, end } => {
                        shift(via);
                        shift(end);
                    }
                    SketchSegment::Bezier {
                        control1,
                        control2,
                        end,
                    } => {
                        shift(control1);
                        shift(control2);
                        shift(end);
                    }
                }
            }
        }
        SketchLoop::Grid { .. } | SketchLoop::Text { .. } => unreachable!(),
    }
    output.push(item);
    Ok(())
}
