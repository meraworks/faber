use crate::path::{Writer, allowance, passes, stepover, validate_stock_depth};
use crate::region::{area, contains, offset, regions_at, silhouette};
use crate::{Region, StockBox, Toolpath};
use faber_model::machining::{Operation, OperationKind, Selection, Side, Tool};
use faber_ports::{Mesh, Result};

pub(crate) fn elevation(selection: &Selection) -> Result<f64> {
    match selection {
        Selection::Z(z) if z.is_finite() => Ok(*z),
        Selection::Z(_) => Err("The selected height must be finite.".into()),
        Selection::Face(_) => Err("Resolve the selected face in setup coordinates first.".into()),
    }
}
pub fn face(stock: &StockBox, tool: &Tool, op: &Operation) -> Result<Toolpath> {
    let OperationKind::Face { stepover: step } = op.kind else {
        return Err("Select a face operation.".into());
    };
    stepover(step, tool.diameter)?;
    let mut path = Writer::new(tool, op)?;
    let h = &op.heights;
    let bottom = h.bottom.unwrap_or(h.top);
    validate_stock_depth(stock, bottom)?;
    let height = stock.max[1] - stock.min[1];
    let count = (height / step).ceil().max(1.);
    if count > 1_000_000. {
        return Err("The face operation exceeds the path limit.".into());
    }
    let (x0, x1) = (
        stock.min[0] - tool.diameter / 2.,
        stock.max[0] + tool.diameter / 2.,
    );
    for z in passes(h.top, bottom, op.stepdown)? {
        let n = count as usize;
        let centre_y = |i: usize| stock.min[1] + (i as f64 + 0.5) * height / count;
        path.start([x0, centre_y(0)])?;
        path.feed([x0, centre_y(0)], z, tool.plunge_mm_min)?;
        for i in 0..n {
            let y = centre_y(i);
            let (a, b) = if i % 2 == 0 { (x0, x1) } else { (x1, x0) };
            path.feed([a, y], z, tool.feed_mm_min)?;
            path.feed([b, y], z, tool.feed_mm_min)?;
        }
    }
    path.finish()
}

pub fn contour(part: &Mesh, tool: &Tool, op: &Operation) -> Result<Toolpath> {
    contour_scoped(part, tool, op, None)
}

/// Follow only the boundary of the selected horizontal face.
pub fn contour_on_face(part: &Mesh, tool: &Tool, op: &Operation, face: &Mesh) -> Result<Toolpath> {
    contour_scoped(part, tool, op, Some(face))
}

fn contour_scoped(
    part: &Mesh,
    tool: &Tool,
    op: &Operation,
    face: Option<&Mesh>,
) -> Result<Toolpath> {
    let OperationKind::Contour {
        z,
        side,
        stock_to_leave,
    } = &op.kind
    else {
        return Err("Select a contour operation.".into());
    };
    let mut path = Writer::new(tool, op)?;
    allowance(*stock_to_leave)?;
    let level = elevation(z)?;
    let section = face.map_or_else(|| regions_at(part, level), silhouette);
    if section.is_empty() {
        return Err("The selected contour height does not cross the part.".into());
    }
    let width = tool.diameter * 0.5 + stock_to_leave;
    let cuts = match side {
        Side::Outside => offset(&section, width),
        Side::Inside => {
            let mut interiors = section
                .iter()
                .flat_map(|r| {
                    r.holes.iter().map(|h| Region {
                        outer: h.clone(),
                        holes: vec![],
                    })
                })
                .collect::<Vec<_>>();
            if interiors.is_empty() {
                interiors = section.clone();
            }
            let mut paths = vec![];
            for r in interiors {
                let inner = offset(&[r], -width);
                if inner.is_empty() {
                    return Err("The tool does not fit the inside contour.".into());
                }
                paths.extend(inner);
            }
            paths
        }
    };
    if cuts.is_empty() {
        return Err("The tool does not fit the contour.".into());
    }
    let h = &op.heights;
    for depth in passes(h.top, h.bottom.unwrap_or(level), op.stepdown)? {
        for part in &cuts {
            path.ring(&part.outer, depth)?;
        }
    }
    path.finish()
}
fn intersects(a: glam::DVec2, b: glam::DVec2, c: glam::DVec2, d: glam::DVec2) -> bool {
    let u = b - a;
    let v = d - c;
    let den = u.perp_dot(v);
    if den.abs() < 1e-12 {
        return false;
    }
    let t = (c - a).perp_dot(v) / den;
    let s = (c - a).perp_dot(u) / den;
    t > 1e-8 && t <= 1. && (0. ..=1.).contains(&s)
}
fn lead(ring: &[[f64; 2]], length: f64, material: &[Region]) -> Result<([f64; 2], Vec<[f64; 2]>)> {
    let mut edges = (0..ring.len()).collect::<Vec<_>>();
    let vec = |i: usize| glam::DVec2::from(ring[i % ring.len()]);
    edges.sort_by(|&a, &b| {
        vec(b)
            .distance_squared(vec(b + 1))
            .total_cmp(&vec(a).distance_squared(vec(a + 1)))
    });
    for i in edges {
        let a = vec(i);
        let b = vec(i + 1);
        let Some(dir) = (b - a).try_normalize() else {
            continue;
        };
        let mid = (a + b) * 0.5;
        let start = mid - glam::DVec2::new(-dir.y, dir.x) * length;
        let in_material = |p: [f64; 2]| {
            material
                .iter()
                .any(|r| contains(&r.outer, p) && !r.holes.iter().any(|h| contains(h, p)))
        };
        if length > 0.
            && (in_material(start.to_array())
                || material.iter().any(|r| {
                    std::iter::once(&r.outer).chain(&r.holes).any(|h| {
                        h.iter()
                            .zip(h.iter().cycle().skip(1))
                            .any(|(&a, &b)| intersects(mid, start, a.into(), b.into()))
                    })
                }))
        {
            continue;
        }
        let mut points = vec![mid.to_array()];
        points.extend((1..=ring.len()).map(|j| ring[(i + j) % ring.len()]));
        points.push(mid.to_array());
        return Ok((start.to_array(), points));
    }
    Err("The lead-in does not fit on the waste side.".into())
}
pub fn cut2d(part: &Mesh, tool: &Tool, op: &Operation) -> Result<Toolpath> {
    let OperationKind::Cut2d {
        kerf,
        lead_in,
        power,
    } = op.kind
    else {
        return Err("Select a 2D cutting operation.".into());
    };
    allowance(kerf)?;
    allowance(lead_in)?;
    allowance(power)?;
    let mut path = Writer::new(tool, op)?;
    let material = silhouette(part);
    let regions = offset(&material, kerf * 0.5);
    if regions.len() != material.len() {
        return Err("The kerf joins separate cut profiles.".into());
    }
    if material.iter().map(|r| r.holes.len()).sum::<usize>()
        != regions.iter().map(|r| r.holes.len()).sum::<usize>()
    {
        return Err("The kerf closes a cut loop.".into());
    }
    let mut rings: Vec<_> = regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .collect();
    rings.sort_by(|a, b| area(a).abs().total_cmp(&area(b).abs()));
    for ring in rings {
        let (start, points) = lead(ring, lead_in, &material)?;
        path.start(start)?;
        path.feed(start, 0., tool.plunge_mm_min)?;
        for p in points {
            path.feed(p, 0., tool.feed_mm_min)?;
        }
    }
    path.finish()
}
