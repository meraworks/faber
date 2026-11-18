use crate::{
    MeshReport, report,
    topology::{self, Topology, WELD_MM},
};
use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use glam::{DMat3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RepairOptions {
    pub weld_mm: f64,
    pub fill_holes: bool,
    pub max_hole_edges: usize,
    pub fix_winding: bool,
    pub drop_shells_below_mm3: Option<f64>,
}
impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            weld_mm: WELD_MM,
            fill_holes: true,
            max_hole_edges: 500,
            fix_winding: true,
            drop_shells_below_mm3: None,
        }
    }
}
#[derive(Default, Clone, Debug, Serialize)]
pub struct RepairLog {
    pub welded: usize,
    pub removed_degenerate: usize,
    pub removed_duplicate: usize,
    pub flipped: usize,
    pub filled_holes: usize,
    pub removed_shells: usize,
    pub left: MeshReport,
}

fn boundaries(mesh: &Mesh, graph: &Topology) -> Vec<Vec<u32>> {
    let mut next = BTreeMap::new();
    let mut ambiguous = HashSet::new();
    for (&(a, b), incident) in &graph.edges {
        let [(face, forward)] = incident.as_slice() else {
            continue;
        };
        let shell = graph.shell_of[*face];
        let (begin, end) = if *forward { (b, a) } else { (a, b) };
        if next.insert((shell, begin), end).is_some() {
            ambiguous.insert(shell);
        }
    }
    let mut seen = HashSet::new();
    let mut loops = Vec::new();
    for &(shell, start) in next.keys() {
        if ambiguous.contains(&shell)
            || !graph.shells[shell].orientable
            || seen.contains(&(shell, start))
        {
            continue;
        }
        let mut cycle = Vec::new();
        let mut v = start;
        while seen.insert((shell, v)) {
            cycle.push(v);
            let Some(&w) = next.get(&(shell, v)) else {
                break;
            };
            v = w;
        }
        if v == start && cycle.len() >= 3 && cycle.len() <= mesh.vertices.len() {
            loops.push(cycle);
        }
    }
    loops
}

/// Fit a plane to all boundary vertices. The smallest covariance eigenvector is its normal.
fn projection(mesh: &Mesh, cycle: &[u32]) -> Option<Vec<DVec2>> {
    let base = DVec3::from(mesh.vertices[cycle[0] as usize]);
    let p: Vec<_> = cycle
        .iter()
        .map(|&i| DVec3::from(mesh.vertices[i as usize]) - base)
        .collect();
    let centre = p.iter().sum::<DVec3>() / p.len() as f64;
    let mut cov = DMat3::ZERO;
    let mut area = DVec3::ZERO;
    for (i, &v) in p.iter().enumerate() {
        let d = v - centre;
        cov += DMat3::from_cols(d * d.x, d * d.y, d * d.z);
        area += d.cross(p[(i + 1) % p.len()] - centre);
    }
    let [x, y, z] = [cov.x_axis, cov.y_axis, cov.z_axis];
    let adj = DMat3::from_cols(y.cross(z), z.cross(x), x.cross(y));
    let mut n = area.try_normalize()?;
    for _ in 0..16 {
        n = (adj * n).try_normalize()?;
    }
    if n.dot(area) < 0. {
        n = -n;
    }
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    Some(p.iter().map(|&p| DVec2::new(p.dot(u), p.dot(v))).collect())
}

fn patch(mesh: &Mesh, cycle: &[u32]) -> Option<Vec<[u32; 3]>> {
    let xy = projection(mesh, cycle)?;
    let coords: Vec<_> = xy.iter().flat_map(|p| p.to_array()).collect();
    let indices = earcutr::earcut(&coords, &[], 2).ok()?;
    let mut triangles = indices.as_chunks::<3>().0.to_vec();
    // A triangulator can omit straight boundary vertices. Split the adjoining
    // triangle so the patch uses the same boundary edges as the open shell.
    let used: HashSet<_> = triangles.iter().flatten().copied().collect();
    let n = cycle.len();
    for i in 0..n {
        if !used.contains(&i) || used.contains(&((i + 1) % n)) {
            continue;
        }
        let mut j = (i + 1) % n;
        while !used.contains(&j) {
            j = (j + 1) % n;
        }
        let (index, k) = triangles.iter().enumerate().find_map(|(t, tri)| {
            (0..3)
                .find(|&k| {
                    tri[k] == i && tri[(k + 1) % 3] == j || tri[k] == j && tri[(k + 1) % 3] == i
                })
                .map(|k| (t, k))
        })?;
        let tri = triangles.swap_remove(index);
        let opposite = tri[(k + 2) % 3];
        let forward = tri[k] == i;
        let mut a = i;
        while a != j {
            let b = (a + 1) % n;
            triangles.push(if forward {
                [a, b, opposite]
            } else {
                [b, a, opposite]
            });
            a = b;
        }
    }
    if triangles.len() + 2 != n {
        return None;
    }
    let mut result = Vec::with_capacity(triangles.len());
    let mut boundary = HashMap::<_, Vec<_>>::new();
    for mut tri in triangles {
        let [a, b, c] = tri.map(|i| xy[i]);
        let area = (b - a).perp_dot(c - a);
        if !area.is_finite() || area == 0. {
            return None;
        }
        if area < 0. {
            tri.swap(0, 1);
        }
        let t = tri.map(|i| cycle[i]);
        for (a, b) in topology::sides(t) {
            boundary
                .entry(topology::key(a, b))
                .or_default()
                .push((a, b));
        }
        result.push(t);
    }
    for i in 0..n {
        let (a, b) = (cycle[i], cycle[(i + 1) % n]);
        if boundary.get(&topology::key(a, b))?.as_slice() != [(a, b)] {
            return None;
        }
    }
    Some(result)
}

struct Rim {
    cycle: Vec<u32>,
    origin: DVec3,
    normal: DVec3,
    area: f64,
    planar: bool,
    low: DVec3,
    high: DVec3,
}
impl Rim {
    fn new(mesh: &Mesh, cycle: Vec<u32>) -> Self {
        let origin = DVec3::from(mesh.vertices[cycle[0] as usize]);
        let mut area = DVec3::ZERO;
        let (mut low, mut high) = (origin, origin);
        for (i, &id) in cycle.iter().enumerate() {
            let a = DVec3::from(mesh.vertices[id as usize]);
            let b = DVec3::from(mesh.vertices[cycle[(i + 1) % cycle.len()] as usize]);
            area += (a - origin).cross(b - origin);
            low = low.min(a);
            high = high.max(a);
        }
        let normal = area.normalize_or_zero();
        let eps = (high - low).length() * 1e-9 + origin.abs().max_element() * 8. * f64::EPSILON;
        let planar = normal != DVec3::ZERO
            && cycle.iter().all(|&i| {
                (DVec3::from(mesh.vertices[i as usize]) - origin)
                    .dot(normal)
                    .abs()
                    <= eps
            });
        Self {
            cycle,
            origin,
            normal,
            area: area.length() * 0.5,
            planar,
            low,
            high,
        }
    }
    fn encloses(&self, mesh: &Mesh, rim: &Rim) -> bool {
        let eps = 1e-8 + (self.high - self.low).length() * 1e-9;
        if !self.planar
            || !rim.planar
            || self.area <= rim.area
            || self.normal.dot(rim.normal).abs() < 1. - 1e-9
            || (self.origin - rim.origin).dot(self.normal).abs() > eps
            || !(self.low - DVec3::splat(eps)).cmple(rim.low).all()
            || !(self.high + DVec3::splat(eps)).cmpge(rim.high).all()
        {
            return false;
        }
        let u = self.normal.any_orthonormal_vector();
        let v = self.normal.cross(u);
        let xy = |id: u32| {
            let d = DVec3::from(mesh.vertices[id as usize]) - rim.origin;
            DVec2::new(d.dot(u), d.dot(v))
        };
        let mut inside = false;
        let mut a = xy(*self.cycle.last().unwrap());
        for &id in &self.cycle {
            let b = xy(id);
            if (a.y > 0.) != (b.y > 0.) && (b.x - a.x) * (-a.y) / (b.y - a.y) + a.x > 0. {
                inside = !inside;
            }
            a = b;
        }
        inside
    }
}
fn fill(mesh: &mut Mesh, limit: usize) -> usize {
    let graph = Topology::new(mesh);
    let rims: Vec<_> = boundaries(mesh, &graph)
        .into_iter()
        .map(|c| Rim::new(mesh, c))
        .collect();
    let mut parents = vec![None; rims.len()];
    let mut depth = vec![0usize; rims.len()];
    for (i, r) in rims.iter().enumerate() {
        let mut nearest = f64::INFINITY;
        for (j, other) in rims.iter().enumerate() {
            if other.encloses(mesh, r) {
                depth[i] += 1;
                if other.area < nearest {
                    parents[i] = Some(j);
                    nearest = other.area;
                }
            }
        }
    }
    let mut count = 0;
    for (i, rim) in rims.iter().enumerate() {
        if !depth[i].is_multiple_of(2) || rim.cycle.len() > limit {
            continue;
        }
        let holes: Vec<_> = parents
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == Some(i))
            .map(|(j, _)| &rims[j])
            .collect();
        if holes
            .iter()
            .any(|h| h.cycle.len() > limit || h.normal.dot(rim.normal) > 0.)
        {
            continue;
        }
        let patch = if holes.is_empty() {
            patch(mesh, &rim.cycle)
        } else {
            let group = std::iter::once(rim.cycle.clone())
                .chain(holes.iter().map(|r| r.cycle.clone()))
                .collect::<Vec<_>>();
            crate::cut::cap_loops(mesh, &group, rim.normal).ok()
        };
        if let Some(triangles) = patch {
            let collision = triangles
                .iter()
                .flat_map(|t| topology::sides(*t))
                .any(|(a, b)| {
                    graph
                        .edges
                        .get(&topology::key(a, b))
                        .is_some_and(|f| f.len() >= 2)
                });
            if !collision {
                mesh.triangles.extend(triangles);
                count += 1;
            }
        }
    }
    count
}

fn closed_shells(mesh: &mut Mesh, options: &RepairOptions, log: &mut RepairLog) -> Result<()> {
    let g = Topology::new(mesh);
    let closed: Vec<_> = g
        .shells
        .iter()
        .filter(|s| s.closed && s.orientable)
        .collect();
    let tree = if options.fix_winding && closed.len() > 1 {
        Some(Bvh::new(&Mesh {
            vertices: mesh.vertices.clone(),
            normals: Vec::new(),
            triangles: closed
                .iter()
                .flat_map(|s| s.faces.iter().map(|&i| mesh.triangles[i]))
                .collect(),
        })?)
    } else {
        None
    };
    let mut remove = vec![false; mesh.triangles.len()];
    for s in closed {
        let volume = topology::volume(mesh, &s.faces);
        if options
            .drop_shells_below_mm3
            .is_some_and(|min| volume.abs() < min)
        {
            for &i in &s.faces {
                remove[i] = true;
            }
            log.removed_shells += 1;
            continue;
        }
        if !options.fix_winding {
            continue;
        }
        let nested = tree.as_ref().is_some_and(|bvh| {
            let [a, b, c] = topology::points(mesh, mesh.triangles[s.faces[0]]);
            let centre = a + (b - a) / 3. + (c - a) / 3.;
            let outward = (b - a).cross(c - a).normalize_or_zero() * volume.signum();
            let eps = (a.distance(b).min(a.distance(c)) * 1e-7)
                .max(centre.abs().max_element() * 32. * f64::EPSILON);
            bvh.inside((centre + outward * eps).into())
        });
        if (volume < 0.) != nested {
            for &i in &s.faces {
                mesh.triangles[i].swap(1, 2);
            }
            log.flipped += s.faces.len();
        }
    }
    let mut i = 0;
    mesh.triangles.retain(|_| {
        i += 1;
        !remove[i - 1]
    });
    Ok(())
}
pub fn try_repair(input: &Mesh, o: &RepairOptions) -> Result<(Mesh, RepairLog)> {
    if o.drop_shells_below_mm3
        .is_some_and(|v| !v.is_finite() || v < 0.)
    {
        return Err("The shell volume limit must be finite and nonnegative.".into());
    }
    let mut mesh = topology::try_weld(input, o.weld_mm)?.mesh;
    let g = Topology::new(&mesh);
    let mut log = RepairLog {
        welded: input.vertices.len() - mesh.vertices.len(),
        removed_degenerate: g.degenerate,
        removed_duplicate: g.duplicate,
        ..Default::default()
    };
    let mut i = 0;
    mesh.triangles.retain_mut(|t| {
        let j = i;
        i += 1;
        if g.drop[j] {
            return false;
        }
        if o.fix_winding && g.flip[j] && g.shells[g.shell_of[j]].orientable {
            t.swap(1, 2);
            log.flipped += 1;
        }
        true
    });
    if o.fill_holes {
        log.filled_holes = fill(&mut mesh, o.max_hole_edges);
    }
    closed_shells(&mut mesh, o, &mut log)?;
    topology::compact(&mut mesh);
    log.left = crate::report::inspected(&mesh)?;
    Ok((mesh, log))
}
pub fn repair(input: &Mesh, options: &RepairOptions) -> (Mesh, RepairLog) {
    try_repair(input, options).unwrap_or_else(|e| {
        let mut left = report(input);
        if !left.problems.contains(&e) {
            left.problems.push(e);
        }
        (
            input.clone(),
            RepairLog {
                left,
                ..Default::default()
            },
        )
    })
}
