//! Constant-depth roughing and straight-line finishing in setup coordinates.
use crate::{
    HeightMap, Region, StockBox, Toolpath,
    height_map::{Cutter, drop_at, tool_surface},
    path::{Writer, passes, stepover},
    pocket::clear,
    validate_stock_depth,
    verify::sweep_cells,
};
use faber_model::machining::{Operation, OperationKind, Tool, ToolKind};
use faber_ports::Result;
use glam::{DVec2, DVec3};
use i_overlay::{core::fill_rule::FillRule, float::simplify::SimplifyShape};
fn mill(tool: &Tool) -> Result<Cutter> {
    if !matches!(tool.kind, ToolKind::Flat | ToolKind::Ball | ToolKind::Bull) {
        return Err("Use a flat, ball or bull mill for a 3D operation.".into());
    }
    Cutter::from_tool(tool)
}
fn zones(tip: &HeightMap, level: f64) -> Vec<Region> {
    let mut rectangles = vec![];
    let xy = |i: usize, j: usize| {
        [
            tip.origin[0] + i as f64 * tip.cell,
            tip.origin[1] + j as f64 * tip.cell,
        ]
    };
    for j in 0..tip.n[1] {
        let mut i = 0;
        while i < tip.n[0] {
            if f64::from(tip.z[j * tip.n[0] + i]) > level + 1e-7 {
                i += 1;
                continue;
            }
            let start = i;
            while i < tip.n[0] && f64::from(tip.z[j * tip.n[0] + i]) <= level + 1e-7 {
                i += 1;
            }
            rectangles.push(vec![xy(start, j), xy(i, j), xy(i, j + 1), xy(start, j + 1)]);
        }
    }
    rectangles
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .filter_map(|loops| {
            let mut l = loops.into_iter();
            Some(Region {
                outer: l.next()?,
                holes: l.collect(),
            })
        })
        .collect()
}
struct SurfacePass<'a> {
    writer: Writer<'a>,
    target: &'a HeightMap,
    cutter: Cutter,
    allowance: f64,
    work: u64,
}
impl SurfacePass<'_> {
    fn stroke(&mut self, points: &[[f64; 2]], floor: f64) -> Result<()> {
        if points.len() < 2 {
            return Ok(());
        }
        let mut samples = vec![];
        let grid = self.target;
        let mut q = DVec2::from(points[0]);
        let tip = |p| drop_at(grid, self.cutter, p, floor - self.allowance) + self.allowance;
        samples.push(q.extend(tip(q)));
        for &p in &points[1..] {
            let end = DVec2::from(p);
            let steps = (q.distance(end) / grid.cell).ceil().max(1.) as usize;
            if steps > 1_000_000 || samples.len() + steps > 1_000_000 {
                return Err("A 3D pass exceeds 1000000 samples.".into());
            }
            for i in 1..=steps {
                let at = q.lerp(end, i as f64 / steps as f64);
                samples.push(at.extend(tip(at)));
            }
            q = end;
        }
        // Contact at each endpoint alone does not bound an inclined move over a ridge.
        let mut lift = vec![0.; samples.len()];
        for (i, pair) in samples.windows(2).enumerate() {
            let mut dz: f64 = 0.;
            sweep_cells(
                grid,
                self.cutter,
                pair[0],
                pair[1],
                &mut self.work,
                |idx, _, z| dz = dz.max(f64::from(grid.z[idx]) + self.allowance - z),
            )?;
            lift[i] = f64::max(lift[i], dz);
            lift[i + 1] = dz;
        }
        self.writer.start(samples[0].truncate().to_array())?;
        let mut prior: Option<(DVec3, DVec3)> = None;
        for (i, (p, z)) in samples.into_iter().zip(lift).enumerate() {
            let dest = DVec3::new(p.x, p.y, p.z + z);
            if let Some((a, b)) = prior {
                let ab: DVec3 = b - a;
                let bc = dest - b;
                if ab.cross(bc).length() > 1e-10 * ab.length().max(1.) || ab.dot(bc) < 0. {
                    self.writer
                        .feed(b.truncate().to_array(), b.z, self.writer.tool.feed_mm_min)?;
                    prior = Some((b, dest));
                } else {
                    prior = Some((a, dest));
                }
            } else {
                if i == 0 {
                    self.writer.feed(
                        p.truncate().to_array(),
                        dest.z,
                        self.writer.tool.plunge_mm_min,
                    )?;
                }
                prior = Some((dest, dest));
            }
        }
        if let Some((_, last)) = prior {
            self.writer.feed(
                last.truncate().to_array(),
                last.z,
                self.writer.tool.feed_mm_min,
            )?;
        }
        Ok(())
    }
}
pub fn rough3d(
    part: &HeightMap,
    stock: &StockBox,
    tool: &Tool,
    op: &Operation,
) -> Result<Toolpath> {
    let OperationKind::Rough3d {
        stepover: step,
        stock_to_leave,
    } = op.kind
    else {
        return Err("Select a 3D roughing operation.".into());
    };
    part.valid()?;
    stock.validate()?;
    stepover(step, tool.diameter)?;
    let tip = tool_surface(part, tool, stock_to_leave)?;
    let bottom = op.heights.bottom.unwrap_or(stock.min[2]);
    validate_stock_depth(stock, bottom)?;
    let mut cutter = mill(tool)?;
    cutter.r += stock_to_leave;
    let mut pass = SurfacePass {
        writer: Writer::new(tool, op)?,
        target: part,
        cutter,
        allowance: stock_to_leave,
        work: 0,
    };
    let levels = passes(op.heights.top, bottom, op.stepdown)?;
    let work = levels.len() as u64 * part.z.len() as u64;
    if work > 400_000_000 {
        return Err(format!(
            "The roughing levels need {work} grid cells. The limit is 400000000."
        ));
    }
    for level in levels {
        for ring in clear(&zones(&tip, level), 0., step)? {
            pass.stroke(&ring, level)?;
        }
    }
    pass.writer.finish()
}
fn ends(lo: DVec2, hi: DVec2, axis: DVec2, at: DVec2) -> Option<[DVec2; 2]> {
    let (mut min, mut max) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..2 {
        if axis[k].abs() < 1e-12 {
            if at[k] < lo[k] - 1e-10 || at[k] > hi[k] + 1e-10 {
                return None;
            }
        } else {
            let a = (lo[k] - at[k]) / axis[k];
            let b = (hi[k] - at[k]) / axis[k];
            min = min.max(a.min(b));
            max = max.min(a.max(b));
        }
    }
    (max - min > 1e-8).then(|| [at + axis * min, at + axis * max])
}
pub fn parallel3d(part: &HeightMap, tool: &Tool, op: &Operation) -> Result<Toolpath> {
    let OperationKind::Parallel3d {
        stepover: step,
        angle_deg,
    } = op.kind
    else {
        return Err("Select a parallel finishing operation.".into());
    };
    part.valid()?;
    stepover(step, tool.diameter)?;
    if !angle_deg.is_finite() {
        return Err("The parallel pass angle must be finite.".into());
    }
    let mut pass = SurfacePass {
        writer: Writer::new(tool, op)?,
        target: part,
        cutter: mill(tool)?,
        allowance: 0.,
        work: 0,
    };
    let floor = f64::from(part.z.iter().copied().fold(f32::INFINITY, f32::min))
        .max(op.heights.bottom.unwrap_or(f64::NEG_INFINITY));
    let axis = DVec2::from_angle(angle_deg.to_radians());
    let cross = axis.perp();
    let lo = DVec2::from(part.origin);
    let hi = lo + DVec2::new(part.n[0] as f64, part.n[1] as f64) * part.cell;
    let dots = [lo, hi, DVec2::new(lo.x, hi.y), DVec2::new(hi.x, lo.y)].map(|p| p.dot(cross));
    let low = dots.into_iter().fold(f64::INFINITY, f64::min);
    let high = dots.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let lines = ((high - low) / step).ceil().max(1.) as usize;
    if lines > 100_000 {
        return Err(format!(
            "The operation needs {lines} parallel passes. The limit is 100000."
        ));
    }
    let mut reverse = false;
    for i in 0..=lines {
        let v = low + (high - low) * i as f64 / lines as f64;
        if let Some(mut pair) = ends(lo, hi, axis, cross * v) {
            if reverse {
                pair.swap(0, 1);
            }
            reverse = !reverse;
            pass.stroke(&pair.map(|p| p.to_array()), floor)?;
        }
    }
    pass.writer.finish()
}
/// Project all portions of the mesh above the cut, including overhangs.
fn material_above(mesh: &faber_ports::Mesh, z: f64) -> Result<Vec<Region>> {
    let mut pieces = Vec::new();
    for tri in &mesh.triangles {
        let [Some(a), Some(b), Some(c)] =
            tri.map(|i| mesh.vertices.get(i as usize).copied().map(DVec3::from))
        else {
            return Err("The mesh has an invalid triangle index.".into());
        };
        let points = [a, b, c];
        if points.iter().any(|p| !p.is_finite()) {
            return Err("The mesh has a non-finite point.".into());
        }
        if points.iter().all(|p| p.z <= z) {
            continue;
        }
        let mut ring = Vec::with_capacity(4);
        let mut prev = c;
        for p in points {
            if (p.z > z && prev.z < z) || (p.z < z && prev.z > z) {
                ring.push(
                    prev.lerp(p, (z - prev.z) / (p.z - prev.z))
                        .truncate()
                        .to_array(),
                );
            }
            if p.z >= z {
                ring.push(p.truncate().to_array());
            }
            prev = p;
        }
        let signed = crate::region::area(&ring);
        if signed < 0. {
            ring.reverse();
        }
        if signed != 0. {
            pieces.push(ring);
        }
    }
    Ok(pieces
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .filter_map(|shape| {
            let mut curves = shape.into_iter();
            Some(Region {
                outer: curves.next()?,
                holes: curves.collect(),
            })
        })
        .collect())
}

/// A flat tip cuts at one height. Retain the facet boundaries at that height
/// so a vertical wall and a narrow ridge keep their actual XY positions.
pub fn rough3d_on_mesh(
    mesh: &faber_ports::Mesh,
    part: &HeightMap,
    stock: &StockBox,
    tool: &Tool,
    op: &Operation,
) -> Result<Toolpath> {
    if tool.kind != ToolKind::Flat {
        return rough3d(part, stock, tool, op);
    }
    let OperationKind::Rough3d {
        stepover: step,
        stock_to_leave,
    } = op.kind
    else {
        return Err("Select a 3D roughing operation.".into());
    };
    stock.validate()?;
    let cutter = mill(tool)?;
    stepover(step, tool.diameter)?;
    crate::path::allowance(stock_to_leave)?;
    let bottom = op.heights.bottom.unwrap_or(stock.min[2]);
    validate_stock_depth(stock, bottom)?;
    let levels = passes(op.heights.top, bottom, op.stepdown)?;
    let total = levels.len() as u64 * mesh.triangles.len() as u64;
    if total > 80_000_000 {
        return Err(format!(
            "The roughing levels need {total} facet tests. The limit is 80000000."
        ));
    }
    let mut writer = Writer::new(tool, op)?;
    let [x, y, _] = stock.min;
    let [u, v, _] = stock.max;
    let bounds = [Region {
        outer: vec![[x, y], [u, y], [u, v], [x, v]],
        holes: vec![],
    }];
    for z in levels {
        let covered = material_above(mesh, z - stock_to_leave)?;
        // The offset arc has a maximum chord error of 0.005 mm.
        let obstacle = crate::offset(&covered, cutter.r + stock_to_leave + 0.005);
        let free = crate::region::boolean(
            &bounds,
            &obstacle,
            i_overlay::core::overlay_rule::OverlayRule::Difference,
        );
        for ring in clear(&free, 0., step.min(cutter.r))? {
            writer.ring(&ring, z)?;
        }
    }
    writer.finish()
}
