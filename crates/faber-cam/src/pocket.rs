use crate::path::{Writer, allowance, passes, stepover, validate_stock_depth};
use crate::region::{Region, Ring, boolean, boundary_distance, contains, offset, regions_at};
use crate::{StockBox, Toolpath};
use faber_model::machining::{Operation, OperationKind, Tool};
use faber_ports::{Mesh, Result};
use glam::DVec2;
use i_overlay::core::overlay_rule::OverlayRule;
use std::f64::consts::TAU;

/// Tool-centre loops, from the centre towards the walls. Each loop closes on its first point.
pub fn clear(region: &[Region], tool_radius: f64, step: f64) -> Result<Vec<Ring>> {
    if !tool_radius.is_finite() || tool_radius < 0. || !step.is_finite() || step <= 0. {
        return Err("The cutter radius and stepover are invalid.".into());
    }
    let mut layers = vec![];
    let mut point_count = 0usize;
    // A step of at most one radius also covers the material left when a narrow ring collapses.
    let spacing = if tool_radius > 0. {
        step.min(tool_radius)
    } else {
        step
    };
    for i in 0..100_000 {
        let mut regions = offset(region, -(tool_radius + f64::from(i) * spacing));
        if regions.is_empty() {
            return Ok(layers.into_iter().rev().flatten().collect());
        }
        let mut layer = vec![];
        for r in &mut regions {
            for mut path in
                std::iter::once(std::mem::take(&mut r.outer)).chain(std::mem::take(&mut r.holes))
            {
                if let Some(&first) = path.first() {
                    path.push(first);
                }
                point_count += path.len();
                if point_count > 1_000_000 {
                    return Err("Pocket clearing exceeds one million points.".into());
                }
                layer.push(path);
            }
        }
        layers.push(layer);
    }
    Err("Pocket clearing exceeds the ring limit.".into())
}
fn width(region: &Region) -> f64 {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for &p in &region.outer {
        lo = lo.min(p.into());
        hi = hi.max(p.into());
    }
    let mut narrow = 0.;
    let mut wide = (hi - lo).min_element() * 0.5;
    for _ in 0..34 {
        let mid = (narrow + wide) * 0.5;
        if offset(std::slice::from_ref(region), -mid).is_empty() {
            wide = mid
        } else {
            narrow = mid
        }
    }
    (narrow * 2. * 1000.).round() / 1000.
}
fn inside(r: &Region, p: [f64; 2]) -> bool {
    contains(&r.outer, p) && !r.holes.iter().any(|h| contains(h, p))
}
fn fit_circle(regions: &[Region], p: DVec2, radius: f64) -> bool {
    regions.iter().any(|r| {
        inside(r, p.into())
            && std::iter::once(&r.outer)
                .chain(&r.holes)
                .all(|ring| boundary_distance(p.into(), ring) + 1e-7 >= radius)
    })
}
fn ramp(w: &mut Writer<'_>, ring: &Ring, limits: &[Region], top: f64, bottom: f64) -> Result<bool> {
    if top <= bottom || ring.len() < 4 {
        return Ok(false);
    }
    let Some((i, edge)) = ring.windows(2).enumerate().max_by(|(_, a), (_, b)| {
        let len = |w: &[[f64; 2]]| DVec2::from(w[0]).distance_squared(w[1].into());
        len(a).total_cmp(&len(b))
    }) else {
        return Ok(false);
    };
    let (a, b) = (DVec2::from(edge[0]), DVec2::from(edge[1]));
    let mid = (a + b) * 0.5;
    let Some(dir) = (b - a).try_normalize() else {
        return Ok(false);
    };
    let normal = DVec2::new(-dir.y, dir.x);
    for radius in [w.tool.diameter * 0.25, w.tool.diameter * 0.125] {
        for side in [1., -1.] {
            let centre = mid + normal * radius * side;
            if !fit_circle(limits, centre, radius) {
                continue;
            }
            // Use straight chords so that the path also works on controls without a helix command.
            let n = (TAU / (2. * (1. - (0.01 / radius).min(1.)).acos()))
                .ceil()
                .max(16.);
            let drop = 2. * radius * (std::f64::consts::PI / n).sin() * 2f64.to_radians().tan();
            let count = (((top - bottom) / drop / n).ceil() * n).max(n);
            if !count.is_finite() || count > 1_000_000. {
                return Err("The helical entry exceeds the move limit.".into());
            }
            w.start(mid.into())?;
            w.feed(mid.into(), top, w.tool.plunge_mm_min)?;
            let start = (mid - centre).to_angle();
            for k in 1..=count as usize {
                let p = if k == count as usize {
                    mid
                } else {
                    centre + DVec2::from_angle(start + TAU * (k as f64 % n) / n) * radius
                };
                w.feed(
                    p.into(),
                    (top - k as f64 * drop).max(bottom),
                    w.tool.plunge_mm_min,
                )?;
            }
            // Finish the entry loop at cutting depth, starting at the tangent point.
            for k in 1..ring.len() {
                w.feed(ring[(i + k) % (ring.len() - 1)], bottom, w.tool.feed_mm_min)?;
            }
            w.feed(mid.into(), bottom, w.tool.feed_mm_min)?;
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn pocket(part: &Mesh, stock: &StockBox, tool: &Tool, op: &Operation) -> Result<Toolpath> {
    pocket_scoped(part, stock, tool, op, None)
}

/// Limit cutting to the selected floor, including the holes in its projection.
pub fn pocket_on_floor(
    part: &Mesh,
    stock: &StockBox,
    tool: &Tool,
    op: &Operation,
    face: &Mesh,
) -> Result<Toolpath> {
    pocket_scoped(part, stock, tool, op, Some(face))
}

fn pocket_scoped(
    part: &Mesh,
    stock: &StockBox,
    tool: &Tool,
    op: &Operation,
    face: Option<&Mesh>,
) -> Result<Toolpath> {
    let OperationKind::Pocket {
        floor,
        stepover: step,
        stock_to_leave,
    } = &op.kind
    else {
        return Err("Select a pocket operation.".into());
    };
    let mut w = Writer::new(tool, op)?;
    stepover(*step, tool.diameter)?;
    allowance(*stock_to_leave)?;
    let floor = crate::two_d::elevation(floor)?;
    let bottom = op.heights.bottom.unwrap_or(floor);
    validate_stock_depth(stock, bottom)?;
    if bottom < floor - 1e-8 {
        return Err("The pocket bottom is below the selected floor.".into());
    }
    // Select the upward-facing floor without subtracting almost coincident bore sections.
    let gap = part
        .vertices
        .iter()
        .map(|p| (p[2] - floor).abs())
        .filter(|d| *d > 1e-8)
        .fold(f64::INFINITY, f64::min);
    let eps = (gap / 4.).min(1e-5);
    let floors = crate::region::horizontal_floor(part, floor);
    let floors = match face {
        Some(face) => boolean(
            &floors,
            &crate::region::silhouette(face),
            OverlayRule::Intersect,
        ),
        None => floors,
    };
    if floors.is_empty() {
        return Err("No horizontal pocket floor exists at the selected height.".into());
    }
    let stock_section = Region {
        outer: vec![
            [stock.min[0], stock.min[1]],
            [stock.max[0], stock.min[1]],
            [stock.max[0], stock.max[1]],
            [stock.min[0], stock.max[1]],
        ],
        holes: vec![],
    };
    let floors = boolean(&floors, &[stock_section], OverlayRule::Intersect);
    let mut previous = op.heights.top;
    for depth in passes(previous, bottom, op.stepdown)? {
        let obstacles = regions_at(part, depth + eps);
        let areas = boolean(&floors, &obstacles, OverlayRule::Difference);
        if areas.is_empty() {
            return Err("The pocket floor is not reachable at this depth.".into());
        }
        for area in areas {
            let cut = offset(std::slice::from_ref(&area), -*stock_to_leave);
            let rings = clear(&cut, tool.diameter * 0.5, *step)?;
            if rings.is_empty() {
                return Err(format!(
                    "Tool diameter {} mm does not fit the pocket width {} mm.",
                    tool.diameter,
                    width(&area)
                ));
            }
            let limits = offset(&cut, -tool.diameter * 0.5);
            for (i, r) in rings.iter().enumerate() {
                if i != 0 || !ramp(&mut w, r, &limits, previous, depth)? {
                    w.ring(r, depth)?;
                }
            }
        }
        previous = depth;
    }
    w.finish()
}
