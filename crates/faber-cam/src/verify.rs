//! Remove the swept tool from a stock height field. Units are millimetres.
use crate::{HeightMap, Move, StockBox, Toolpath, height_map::Cutter};
use faber_model::machining::{Tool, ToolKind};
use faber_ports::{Mesh, Result};
use glam::{DVec2, DVec3};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct Verification {
    pub gouge_mm: f64,
    pub gouge_at: Option<[f64; 3]>,
    pub max_excess_mm: f64,
    pub removed_mm3: f64,
    pub rapid_collision_count: usize,
    pub rapid_collisions: Vec<[f64; 3]>,
    pub warnings: Vec<String>,
    pub minutes: f64,
    pub stock: HeightMap,
    pub stock_bounds: StockBox,
}
#[derive(Clone, Debug, Default)]
pub struct OperationVerification {
    pub operation: String,
    pub tool: String,
    pub gouge_mm: f64,
    pub max_excess_mm: f64,
    pub removed_mm3: f64,
    pub rapid_collision_count: usize,
    pub rapid_collisions: Vec<[f64; 3]>,
    pub warnings: Vec<String>,
    pub minutes: f64,
}

// Lowest point of the tool above q for the complete linear move.
fn swept(c: Cutter, a: DVec3, b: DVec3, q: DVec2) -> Option<f64> {
    let d = b - a;
    let len = d.truncate().length();
    if len < 1e-12 {
        return Some(a.z.min(b.z) + c.height(q.distance_squared(a.truncate()))?);
    }
    let axis = d.truncate() / len;
    let rel = q - a.truncate();
    let along = rel.dot(axis);
    let across = rel.perp_dot(axis).abs();
    if across > c.r {
        return None;
    }
    let reach = (c.r * c.r - across * across).max(0.).sqrt();
    let l = (along - reach).max(0.);
    let h = (along + reach).min(len);
    if l > h {
        return None;
    }
    let slope = d.z / len;
    if c.corner == 0. {
        return Some(a.z + slope * (if slope > 0. { l } else { h }));
    }
    let z = |s: f64| {
        a.z + slope * s
            + c.height((across * across + (s - along).powi(2)).min(c.r * c.r))
                .unwrap_or(c.corner)
    };
    if c.r == c.corner {
        let min = along - slope * reach / (1. + slope * slope).sqrt();
        return Some(z(min.clamp(l, h)));
    }
    // The bull nose profile is convex in this interval, including its flat centre.
    let (mut left, mut right) = (l, h);
    for _ in 0..30 {
        let p = (2. * left + right) / 3.;
        let q = (left + 2. * right) / 3.;
        if z(p) < z(q) {
            right = q;
        } else {
            left = p;
        }
    }
    Some(z((left + right) / 2.).min(z(l)).min(z(h)))
}
const MAX_VISITS: u64 = 800_000_000;
pub(crate) fn sweep_cells(
    grid: &HeightMap,
    c: Cutter,
    a: DVec3,
    b: DVec3,
    work: &mut u64,
    mut visit: impl FnMut(usize, DVec2, f64),
) -> Result<()> {
    let len = (b - a).truncate().length();
    let sections = (len / (4. * c.r.max(grid.cell))).ceil().max(1.);
    if !sections.is_finite() || sections > 2_000_000. {
        return Err(format!(
            "The move needs {sections} sweep sections. The limit is 2000000."
        ));
    }
    let sections = sections as usize;
    for i in 0..sections {
        let first = a.lerp(b, i as f64 / sections as f64);
        let last = a.lerp(b, (i + 1) as f64 / sections as f64);
        let lo = first.truncate().min(last.truncate()) - c.r;
        let hi = first.truncate().max(last.truncate()) + c.r;
        let [x0, x1, y0, y1] = grid.window(lo, hi);
        *work += ((x1 - x0 + 1) * (y1 - y0 + 1)) as u64;
        if *work > MAX_VISITS {
            return Err(format!(
                "The swept tool needs at least {work} cell tests. The limit is {MAX_VISITS}."
            ));
        }
        for j in y0..=y1 {
            for k in x0..=x1 {
                let q = grid.point(k, j);
                if let Some(z) = swept(c, first, last, q) {
                    visit(j * grid.n[0] + k, q, z);
                }
            }
        }
    }
    Ok(())
}
pub fn verify(
    part: &HeightMap,
    stock: &StockBox,
    paths: &[(Toolpath, Tool)],
    tolerance: f64,
) -> Result<Verification> {
    Ok(verify_operations(part, stock, paths, tolerance)?.0)
}
pub fn verify_operations(
    part: &HeightMap,
    stock: &StockBox,
    paths: &[(Toolpath, Tool)],
    tolerance: f64,
) -> Result<(Verification, Vec<OperationVerification>)> {
    stock.validate()?;
    part.valid()?;
    if !tolerance.is_finite() || tolerance < 0. {
        return Err("The verification tolerance must be finite and non-negative.".into());
    }
    let mut grid = crate::height_map::empty(
        [stock.min[0], stock.min[1]],
        [stock.max[0], stock.max[1]],
        part.cell,
        stock.max[2],
    )?;
    let target: Vec<_> = (0..grid.z.len())
        .map(|i| part.sample(grid.point(i % grid.n[0], i / grid.n[0]), stock.min[2]))
        .collect();
    let mut heights = grid.z.clone();
    let mut work = 0;
    let mut reports = Vec::new();
    let mut messages = BTreeSet::new();
    let mut count = 0;
    let mut contacts = vec![];
    let mut max_gouge = 0.;
    let mut gouge_at = None;
    let mut removed = 0.;
    let mut minutes = 0.;
    let mut prev: Option<DVec3> = None;
    let mut excess = target
        .iter()
        .map(|p| (stock.max[2] - p).max(0.))
        .fold(0., f64::max);
    for (path, t) in paths {
        let cutter = Cutter::from_tool(t)?;
        let laser = t.kind == ToolKind::Laser;
        let mut lowest = stock.max[2];
        let mut r = OperationVerification {
            operation: path.operation.clone(),
            tool: path.tool.clone(),
            ..Default::default()
        };
        for (move_index, mv) in path.moves.iter().enumerate() {
            let (to, feed, rapid) = match *mv {
                Move::Dwell { seconds } => {
                    if !seconds.is_finite() || seconds < 0. {
                        return Err("Invalid dwell time.".into());
                    }
                    r.minutes += seconds / 60.;
                    continue;
                }
                Move::Rapid { to } => (to, 3000., true),
                Move::Feed { to, feed } => (to, feed, false),
            };
            let to = DVec3::from(to);
            if !to.is_finite() || !feed.is_finite() || feed <= 0. {
                return Err("A move needs finite coordinates and a positive feed.".into());
            }
            let start = prev.unwrap_or(to);
            prev = Some(to);
            let corner = DVec3::new(start.x, start.y, to.z);
            let legs = [(start, corner), (corner, to)];
            let direct = [(start, to)];
            let segments = if move_index == 0
                && rapid
                && !laser
                && start.z != to.z
                && start.truncate() != to.truncate()
            {
                &legs[..]
            } else {
                &direct[..]
            };
            for &(from, to) in segments {
                r.minutes += (if laser {
                    from.truncate().distance(to.truncate())
                } else {
                    from.distance(to)
                }) / feed;
                if laser && rapid {
                    continue;
                }
                let mut collision = None;
                let (a, b) = if laser {
                    (
                        DVec3::new(from.x, from.y, stock.min[2]),
                        DVec3::new(to.x, to.y, stock.min[2]),
                    )
                } else {
                    (from, to)
                };
                if !rapid {
                    lowest = lowest.min(a.z).min(b.z);
                }
                sweep_cells(&grid, cutter, a, b, &mut work, |i, q, z| {
                    let old = f64::from(heights[i]);
                    if rapid {
                        if collision.is_none() && z < old - tolerance {
                            collision = Some([q.x, q.y, z]);
                        }
                        return;
                    }
                    let gap = target[i] - z;
                    if gap > tolerance {
                        r.gouge_mm = r.gouge_mm.max(gap);
                        if gap > max_gouge {
                            max_gouge = gap;
                            gouge_at = Some([q.x, q.y, z]);
                        }
                    }
                    let new = old.min(z.max(stock.min[2])) as f32;
                    if new < heights[i] {
                        let corner = grid.point(i % grid.n[0], i / grid.n[0]) - grid.cell * 0.5;
                        let width = (DVec2::new(stock.max[0], stock.max[1]) - corner)
                            .clamp(DVec2::ZERO, DVec2::splat(grid.cell));
                        r.removed_mm3 += (old - f64::from(new)) * width.x * width.y;
                        heights[i] = new;
                    }
                })?;
                if let Some(p) = collision {
                    count += 1;
                    r.rapid_collision_count += 1;
                    if r.rapid_collisions.len() < 128 {
                        r.rapid_collisions.push(p);
                    }
                    if contacts.len() < 128 {
                        contacts.push(p);
                    }
                }
            }
        }
        let depth = stock.max[2] - lowest;
        if !laser && depth > t.flute_length + 1e-8 {
            r.warnings.push(format!(
                "Cut depth {depth} mm is more than the flute length {} mm of tool {}.",
                t.flute_length, path.tool
            ));
        }
        excess = heights
            .iter()
            .zip(&target)
            .map(|(&z, &p)| (f64::from(z) - p).max(0.))
            .fold(0., f64::max);
        r.max_excess_mm = excess;
        messages.extend(r.warnings.iter().cloned());
        removed += r.removed_mm3;
        minutes += r.minutes;
        reports.push(r);
    }
    grid.z = heights;
    Ok((
        Verification {
            gouge_mm: max_gouge,
            gouge_at,
            max_excess_mm: excess,
            removed_mm3: removed,
            rapid_collision_count: count,
            rapid_collisions: contacts,
            warnings: messages.into_iter().collect(),
            minutes,
            stock: grid,
            stock_bounds: *stock,
        },
        reports,
    ))
}
/// Keep the highest remaining material in each display block.
pub fn stock_mesh(v: &Verification) -> Mesh {
    let field = &v.stock;
    let [nx, ny] = field.n;
    let stride = nx.max(ny).div_ceil(256).max(1);
    let (w, h) = (nx.div_ceil(stride), ny.div_ceil(stride));
    let bottom = v.stock_bounds.min[2];
    let mut z = vec![bottom; w * h];
    for j in 0..ny {
        for i in 0..nx {
            let k = j / stride * w + i / stride;
            z[k] = z[k].max(f64::from(field.z[j * nx + i]));
        }
    }
    let mut mesh = Mesh::default();
    let mut quad = |p: [[f64; 3]; 4]| {
        let k = mesh.vertices.len() as u32;
        mesh.vertices.extend(p);
        mesh.triangles
            .extend([[k, k + 1, k + 2], [k, k + 2, k + 3]]);
    };
    for j in 0..h {
        for i in 0..w {
            let x = |k: usize| {
                (field.origin[0] + (k * stride) as f64 * field.cell).min(v.stock_bounds.max[0])
            };
            let y = |k: usize| {
                (field.origin[1] + (k * stride) as f64 * field.cell).min(v.stock_bounds.max[1])
            };
            let top = z[j * w + i];
            if top <= bottom {
                continue;
            }
            let corners = [
                [x(i), y(j)],
                [x(i + 1), y(j)],
                [x(i + 1), y(j + 1)],
                [x(i), y(j + 1)],
            ];
            quad(corners.map(|[x, y]| [x, y, top]));
            let [a, b, c, d] = corners;
            quad([d, c, b, a].map(|[x, y]| [x, y, bottom]));
            let neighbours = [
                j.checked_sub(1).map(|jj| jj * w + i),
                (i + 1 < w).then_some(j * w + i + 1),
                (j + 1 < h).then_some((j + 1) * w + i),
                i.checked_sub(1).map(|ii| j * w + ii),
            ];
            for (e, other) in neighbours.into_iter().enumerate() {
                let lower = other.map_or(bottom, |k| z[k]);
                if lower >= top {
                    continue;
                }
                let [x0, y0] = corners[e];
                let [x1, y1] = corners[(e + 1) % 4];
                quad([
                    [x0, y0, lower],
                    [x1, y1, lower],
                    [x1, y1, top],
                    [x0, y0, top],
                ]);
            }
        }
    }
    mesh
}
