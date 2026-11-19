//! Highest material in each square XY cell. Z and XY use setup coordinates.
use crate::StockBox;
use faber_geometry::bvh::Bvh;
use faber_model::machining::{Tool, ToolKind};
use faber_ports::{Mesh, Result};
use glam::{DVec2, DVec3};
use rayon::prelude::*;

const MAX_CELLS: usize = 16_000_000;
const MAX_TESTS: u64 = 800_000_000;
#[derive(Clone, Debug)]
pub struct HeightMap {
    pub origin: [f64; 2],
    pub cell: f64,
    pub n: [usize; 2],
    pub z: Vec<f32>,
}
impl HeightMap {
    pub(crate) fn valid(&self) -> Result<()> {
        if !self.cell.is_finite()
            || self.cell <= 0.
            || self.origin.iter().any(|x| !x.is_finite())
            || self.n.contains(&0)
            || self.n[0].checked_mul(self.n[1]) != Some(self.z.len())
            || self.z.iter().any(|x| !x.is_finite())
        {
            return Err("The height map has invalid coordinates, heights or dimensions.".into());
        }
        if self.z.len() > MAX_CELLS {
            return Err(format!(
                "The height map needs {} cells. The limit is {MAX_CELLS}.",
                self.z.len()
            ));
        }
        Ok(())
    }
    pub(crate) fn point(&self, x: usize, y: usize) -> DVec2 {
        DVec2::from(self.origin) + self.cell * DVec2::new(x as f64 + 0.5, y as f64 + 0.5)
    }
    fn cell_index(&self, x: f64, k: usize) -> usize {
        (((x - self.origin[k]) / self.cell).floor() as isize).clamp(0, self.n[k] as isize - 1)
            as usize
    }
    pub(crate) fn window(&self, lo: DVec2, hi: DVec2) -> [usize; 4] {
        [
            self.cell_index(lo.x, 0),
            self.cell_index(hi.x, 0),
            self.cell_index(lo.y, 1),
            self.cell_index(hi.y, 1),
        ]
    }
    pub(crate) fn sample(&self, p: DVec2, default: f64) -> f64 {
        let v = (p - DVec2::from(self.origin)) / self.cell;
        if v.min_element() < 0. || v.x >= self.n[0] as f64 || v.y >= self.n[1] as f64 {
            default
        } else {
            f64::from(self.z[v.y as usize * self.n[0] + v.x as usize])
        }
    }
}
pub(crate) fn empty(low: [f64; 2], high: [f64; 2], cell: f64, floor: f64) -> Result<HeightMap> {
    if !cell.is_finite()
        || cell <= 0.
        || !(floor as f32).is_finite()
        || low.iter().chain(&high).any(|x| !x.is_finite())
    {
        return Err("Use finite grid coordinates and a positive cell size.".into());
    }
    let dims = [0, 1].map(|i| ((high[i] - low[i]) / cell).ceil().max(1.));
    let count = dims[0] * dims[1];
    if !count.is_finite() || count > MAX_CELLS as f64 {
        return Err(format!(
            "The height map needs {count:.0} cells. The limit is {MAX_CELLS}."
        ));
    }
    Ok(HeightMap {
        origin: low,
        n: dims.map(|v| v as usize),
        cell,
        z: vec![floor as f32; count as usize],
    })
}
// Clip the triangle against a cell. A linear facet reaches its highest point at a vertex.
fn cell_top(tri: [DVec3; 3], lo: DVec2, cell: f64) -> Option<f64> {
    let mut a = [DVec3::ZERO; 8];
    a[..3].copy_from_slice(&tri);
    let mut n = 3;
    for (k, bound, sign) in [
        (0, lo.x, 1.),
        (0, lo.x + cell, -1.),
        (1, lo.y, 1.),
        (1, lo.y + cell, -1.),
    ] {
        if n < 3 {
            return None;
        }
        let mut b = [DVec3::ZERO; 8];
        let mut count = 0;
        let mut prev = a[n - 1];
        let mut pd = sign * (prev[k] - bound);
        for &p in &a[..n] {
            let d = sign * (p[k] - bound);
            if (d > 0. && pd < 0.) || (d < 0. && pd > 0.) {
                b[count] = prev.lerp(p, pd / (pd - d));
                count += 1;
            }
            if d >= 0. {
                b[count] = p;
                count += 1;
            }
            prev = p;
            pd = d;
        }
        a = b;
        n = count;
    }
    if n < 3 {
        return None;
    }
    let base = a[0].truncate();
    let area = (1..n - 1)
        .map(|i| (a[i].truncate() - base).perp_dot(a[i + 1].truncate() - base))
        .sum::<f64>();
    (area != 0.).then(|| a[..n].iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max))
}
fn raster(mesh: &Mesh, mut map: HeightMap) -> Result<HeightMap> {
    let mut work = 0u64;
    for tri in &mesh.triangles {
        let [a, b, c] = tri.map(|i| mesh.vertices.get(i as usize).copied().map(DVec3::from));
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            return Err("Invalid mesh triangle index.".into());
        };
        if !a.is_finite() || !b.is_finite() || !c.is_finite() {
            return Err("Mesh coordinates must be finite.".into());
        }
        if (b - a).truncate().perp_dot((c - a).truncate()) == 0. {
            continue;
        }
        let lo = a.min(b).min(c);
        let hi = a.max(b).max(c);
        let [x0, x1, y0, y1] = map.window(lo.truncate(), hi.truncate());
        work += ((x1 - x0 + 1) * (y1 - y0 + 1)) as u64;
        if work > MAX_TESTS {
            return Err(format!(
                "Rasterisation needs at least {work} cell tests. The limit is {MAX_TESTS}."
            ));
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = y * map.n[0] + x;
                if f64::from(map.z[i]) >= hi.z {
                    continue;
                }
                let corner = DVec2::from(map.origin) + map.cell * DVec2::new(x as f64, y as f64);
                if let Some(z) = cell_top([a, b, c], corner, map.cell) {
                    if !(z as f32).is_finite() {
                        return Err("A mesh height exceeds the height map range.".into());
                    }
                    map.z[i] = map.z[i].max(z as f32);
                }
            }
        }
    }
    Ok(map)
}
pub fn stock_height_map(mesh: &Mesh, _bvh: &Bvh, stock: &StockBox, cell: f64) -> Result<HeightMap> {
    stock.validate()?;
    raster(
        mesh,
        empty(
            [stock.min[0], stock.min[1]],
            [stock.max[0], stock.max[1]],
            cell,
            stock.min[2],
        )?,
    )
}
pub fn height_map(mesh: &Mesh, _bvh: &Bvh, cell: f64, floor: f64) -> Result<HeightMap> {
    let mut low = DVec2::splat(f64::INFINITY);
    let mut high = -low;
    for id in mesh.triangles.iter().flatten() {
        let p = mesh
            .vertices
            .get(*id as usize)
            .ok_or("Invalid mesh triangle index.")?;
        let xy = DVec2::new(p[0], p[1]);
        low = low.min(xy);
        high = high.max(xy);
    }
    raster(mesh, empty(low.to_array(), high.to_array(), cell, floor)?)
}
#[derive(Clone, Copy)]
pub(crate) struct Cutter {
    pub r: f64,
    pub corner: f64,
}
impl Cutter {
    pub fn from_tool(t: &Tool) -> Result<Self> {
        let r = t.diameter * 0.5;
        if !r.is_finite() || r <= 0. {
            return Err("The tool needs a positive finite diameter.".into());
        }
        let corner = match t.kind {
            ToolKind::Ball => r,
            ToolKind::Bull => t.corner_radius,
            _ => 0.,
        };
        if !corner.is_finite() || corner < 0. || corner > r {
            return Err("The corner radius must be between zero and the tool radius.".into());
        }
        Ok(Self { r, corner })
    }
    pub fn height(self, d2: f64) -> Option<f64> {
        if d2 > self.r * self.r + 1e-12 {
            return None;
        }
        if self.corner == 0. {
            return Some(0.);
        }
        let d = (d2.sqrt() - (self.r - self.corner)).max(0.);
        Some(self.corner - (self.corner * self.corner - d * d).max(0.).sqrt())
    }
}
pub(crate) fn drop_at(map: &HeightMap, c: Cutter, q: DVec2, floor: f64) -> f64 {
    let [x0, x1, y0, y1] = map.window(q - c.r, q + c.r);
    let mut z = floor;
    for j in y0..=y1 {
        for i in x0..=x1 {
            let sample = f64::from(map.z[j * map.n[0] + i]);
            if sample > z
                && let Some(h) = c.height(q.distance_squared(map.point(i, j)))
            {
                z = z.max(sample - h);
            }
        }
    }
    z
}
pub fn tool_surface(map: &HeightMap, tool: &Tool, stock_to_leave: f64) -> Result<HeightMap> {
    map.valid()?;
    let mut c = Cutter::from_tool(tool)?;
    if !stock_to_leave.is_finite() || stock_to_leave < 0. {
        return Err("Stock to leave must be finite and non-negative.".into());
    }
    c.r += stock_to_leave;
    let width = (2. * (c.r / map.cell).ceil() + 1.).min(map.n[0].max(map.n[1]) as f64);
    let checks = map.z.len() as f64 * width * width;
    if checks > MAX_TESTS as f64 {
        return Err(format!(
            "The cutter envelope needs up to {checks:.0} cell tests. The limit is {MAX_TESTS}."
        ));
    }
    let floor = map.z.iter().copied().fold(f32::INFINITY, f32::min) as f64;
    let z = (0..map.z.len())
        .into_par_iter()
        .map(|i| {
            (drop_at(map, c, map.point(i % map.n[0], i / map.n[0]), floor) + stock_to_leave) as f32
        })
        .collect();
    Ok(HeightMap {
        z,
        n: map.n,
        origin: map.origin,
        cell: map.cell,
    })
}
pub fn hidden_area(mesh: &Mesh, bvh: &Bvh, up: [f64; 3]) -> f64 {
    let Some(up) = DVec3::from(up).try_normalize() else {
        return 0.;
    };
    let bottom = mesh
        .triangles
        .iter()
        .flatten()
        .map(|&i| DVec3::from(mesh.vertices[i as usize]).dot(up))
        .fold(f64::INFINITY, f64::min);
    mesh.triangles
        .par_iter()
        .map(|t| {
            let pts = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
            let [a, b, c] = pts;
            let area = (b - a).cross(c - a).length() / 2.;
            if pts.iter().all(|p| p.dot(up) <= bottom + 1e-7) {
                return 0.;
            }
            let blocked = [
                [2. / 3., 1. / 6., 1. / 6.],
                [1. / 6., 2. / 3., 1. / 6.],
                [1. / 6., 1. / 6., 2. / 3.],
            ]
            .into_iter()
            .filter(|w| {
                bvh.first_hit(
                    (a * w[0] + b * w[1] + c * w[2] + up * 1e-7).to_array(),
                    up.to_array(),
                )
                .is_some()
            })
            .count();
            area * blocked as f64 / 3.
        })
        .sum()
}

// Facets on the annular face touch or miss the inscribed rim polygon. A
// cover and a blind-hole floor intersect its interior and remain material.
fn rim_polygon(mesh: &Mesh, centre: DVec2, r: f64, level: f64, eps: f64) -> Vec<DVec2> {
    let mut ring: Vec<_> = mesh
        .vertices
        .iter()
        .map(|p| DVec3::from(*p))
        .filter(|p| (p.z - level).abs() <= eps)
        .map(|p| p.truncate() - centre)
        .filter(|p| (p.length() - r).abs() <= eps)
        .collect();
    ring.sort_by(|a, b| a.y.atan2(a.x).total_cmp(&b.y.atan2(b.x)));
    ring.dedup_by(|a, b| a.distance(*b) <= eps);
    ring
}
fn outside_rim(tri: [DVec2; 3], ring: &[DVec2], eps: f64) -> bool {
    if ring.len() < 3 {
        return false;
    }
    // Separating axes of convex polygons include the normal of every edge.
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let e = *b - *a;
        if tri.iter().all(|p| e.perp_dot(*p - *a) <= eps * e.length()) {
            return true;
        }
    }
    for (a, b) in tri.iter().zip(tri.iter().cycle().skip(1)) {
        let d = (*b - *a).perp();
        if d.length_squared() < eps * eps {
            continue;
        }
        let project = |ps: &[DVec2]| {
            ps.iter()
                .map(|p| p.dot(d))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v), hi.max(v))
                })
        };
        let (al, ah) = project(&tri);
        let (bl, bh) = project(ring);
        if ah <= bl + eps * d.length() || bh <= al + eps * d.length() {
            return true;
        }
    }
    false
}
/// Sample the surface at each verification ray. A closed cylindrical wall has
/// a circular rim even when its display triangles have a coarse chord.
pub fn verification_target(
    mesh: &Mesh,
    bvh: &Bvh,
    stock: &StockBox,
    cell: f64,
    bores: &[crate::Hole],
) -> Result<HeightMap> {
    stock.validate()?;
    let mut grid = empty(
        [stock.min[0], stock.min[1]],
        [stock.max[0], stock.max[1]],
        cell,
        stock.min[2],
    )?;
    let highest = mesh
        .vertices
        .iter()
        .map(|p| p[2])
        .fold(stock.max[2], f64::max);
    let start = highest + 1. + highest.abs() * 1e-12;
    let down = [0., 0., -1.];
    let [nx, _] = grid.n;
    let origin = DVec2::from(grid.origin);
    grid.z.par_iter_mut().enumerate().for_each(|(i, z)| {
        let q = origin + DVec2::new((i % nx) as f64 + 0.5, (i / nx) as f64 + 0.5) * cell;
        if let Some(h) = bvh.first_hit([q.x, q.y, start], down) {
            *z = (h.point[2].max(stock.min[2])) as f32;
        }
    });
    let mut work = 0u64;
    for bore in bores {
        let centre = DVec2::from(bore.centre);
        let r = bore.diameter / 2.;
        if !r.is_finite()
            || r <= 0.
            || !centre.is_finite()
            || !bore.top.is_finite()
            || !bore.bottom.is_finite()
        {
            return Err("The bore has invalid dimensions.".into());
        }
        let [x0, x1, y0, y1] = grid.window(centre - r, centre + r);
        work += ((x1 - x0 + 1) * (y1 - y0 + 1)) as u64;
        if work > MAX_TESTS {
            return Err(format!(
                "The bore correction needs at least {work} cell tests. The limit is {MAX_TESTS}."
            ));
        }
        let eps = 1e-7 * r.max(1.);
        work += 2 * mesh.vertices.len() as u64;
        if work > MAX_TESTS {
            return Err(format!(
                "The bore correction needs at least {work} checks. The limit is {MAX_TESTS}."
            ));
        }
        let top_ring = rim_polygon(mesh, centre, r, bore.top, eps);
        let bottom_ring = rim_polygon(mesh, centre, r, bore.bottom, eps);
        for j in y0..=y1 {
            for i in x0..=x1 {
                let idx = j * nx + i;
                let q = grid.point(i, j);
                if q.distance_squared(centre) > r * r
                    || (f64::from(grid.z[idx]) - bore.top).abs()
                        > eps.max((bore.top as f32).abs() as f64 * 1.2e-7)
                {
                    continue;
                }
                let Some(hit) = bvh.first_hit([q.x, q.y, start], down) else {
                    continue;
                };
                let is_rim_facet = |id: u32| {
                    let tri =
                        mesh.triangles[id as usize].map(|v| DVec3::from(mesh.vertices[v as usize]));
                    // Do not remove a cap over a blind bore, or a separate part above it.
                    let on_level = |z: f64| tri.iter().all(|v| (v.z - z).abs() <= eps);
                    let ring = if on_level(bore.top) {
                        &top_ring
                    } else if on_level(bore.bottom) {
                        &bottom_ring
                    } else {
                        return false;
                    };
                    outside_rim(tri.map(|p| p.truncate() - centre), ring, eps)
                };
                if !is_rim_facet(hit.triangle) {
                    continue;
                }
                grid.z[idx] = bvh
                    .hits([q.x, q.y, start], down)
                    .iter()
                    .find(|h| !is_rim_facet(h.triangle))
                    .map_or(stock.min[2], |h| h.point[2].max(stock.min[2]))
                    as f32;
            }
        }
    }
    Ok(grid)
}
