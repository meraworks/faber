use faber_ports::{Mesh, Result};
use glam::{DQuat, DVec3};
use std::collections::HashMap;
use std::ops::Range;

const TRIANGLE_LIMIT: usize = 2_000_000;
const NONE: usize = usize::MAX;
pub(crate) struct Facet {
    pub n: DVec3,
    pub area: f64,
    pub centre: DVec3,
}
pub(crate) struct Surface {
    pub mesh: Mesh,
    pub facets: Vec<Facet>,
    pub adjacent: Vec<[usize; 3]>,
    pub parts: Vec<Range<usize>>,
    pub area: f64,
    pub volume: f64,
    pub eps: f64,
}
struct EdgeUse {
    triangle: usize,
    side: usize,
    from: u32,
    count: usize,
    same_direction: bool,
}

pub(crate) fn direction(v: [f64; 3]) -> Result<DVec3> {
    DVec3::from(v)
        .try_normalize()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "The build direction must be finite and nonzero.".into())
}
pub(crate) fn turn(up: DVec3) -> DQuat {
    DQuat::from_rotation_arc(up, DVec3::Z)
}
pub(crate) fn extent(vertices: &[[f64; 3]], q: DQuat) -> (DVec3, DVec3) {
    vertices
        .iter()
        .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), &p| {
            let v = q * DVec3::from(p);
            (lo.min(v), hi.max(v))
        })
}
pub(crate) fn geometry(mesh: &Mesh, tri: [u32; 3]) -> Facet {
    let [a, b, c] = tri.map(|i| DVec3::from(mesh.vertices[i as usize]));
    let cross = (b - a).cross(c - a);
    Facet {
        n: cross.normalize_or_zero(),
        area: cross.length() / 2.,
        centre: a + (b - a) / 3. + (c - a) / 3.,
    }
}
pub(crate) fn ray_offset(mesh: &Mesh) -> f64 {
    let (lo, hi) = extent(&mesh.vertices, DQuat::IDENTITY);
    ((hi - lo).max_element() * 1e-10).max(lo.abs().max(hi.abs()).max_element() * 16. * f64::EPSILON)
}
pub(crate) fn signed_volume(mesh: &Mesh) -> f64 {
    let Some(first) = mesh.triangles.first() else {
        return 0.;
    };
    let origin = mesh.vertices[first[0] as usize];
    mesh.triangles
        .iter()
        .map(|tri| {
            let [a, b, c] =
                tri.map(|i| DVec3::from(mesh.vertices[i as usize]) - DVec3::from(origin));
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}

impl Surface {
    pub fn new(meshes: &[&Mesh]) -> Result<Self> {
        let count = meshes
            .iter()
            .fold(0usize, |n, m| n.saturating_add(m.triangles.len()));
        if count == 0 {
            return Err("The part has no mesh surface.".into());
        }
        if count > TRIANGLE_LIMIT {
            return Err(format!(
                "The mesh has {count} triangles. The limit is {TRIANGLE_LIMIT}. Use a coarser mesh."
            ));
        }
        let mut mesh = Mesh {
            vertices: vec![],
            triangles: Vec::with_capacity(count),
            normals: vec![],
        };
        let mut adjacent = vec![[NONE; 3]; count];
        let mut parts = Vec::new();
        let mut volume = 0.;
        for source in meshes {
            let start = mesh.triangles.len();
            if source.vertices.is_empty() || source.triangles.is_empty() {
                return Err("The part has no mesh surface.".into());
            }
            let (vertices, indices) = source.welded_vertices(1e-6)?;
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend(vertices);
            let mut edges = HashMap::new();
            for &triangle in &source.triangles {
                let tri = triangle.map(|i| base + indices[i as usize]);
                let i = mesh.triangles.len();
                mesh.triangles.push(tri);
                if geometry(&mesh, tri).area == 0. {
                    continue;
                }
                for j in 0..3 {
                    let a = tri[j];
                    let b = tri[(j + 1) % 3];
                    let edge = edges.entry((a.min(b), a.max(b))).or_insert(EdgeUse {
                        triangle: i,
                        side: j,
                        from: a,
                        count: 0,
                        same_direction: false,
                    });
                    edge.count += 1;
                    if edge.count == 2 {
                        adjacent[edge.triangle][edge.side] = i;
                        adjacent[i][j] = edge.triangle;
                        edge.same_direction = edge.from == a;
                    }
                }
            }
            let open = edges.values().filter(|e| e.count == 1).count();
            if open > 0 {
                return Err(format!(
                    "The mesh is open ({open} open edges). Repair it first."
                ));
            }
            let bad = edges
                .values()
                .filter(|e| e.count != 2 || e.same_direction)
                .count();
            if bad > 0 {
                return Err(format!(
                    "The mesh has {bad} non-manifold or misoriented edges. Repair it first."
                ));
            }
            let v = signed_volume(source);
            if !v.is_finite() || v == 0. {
                return Err("The mesh has no finite enclosed volume.".into());
            }
            let end = mesh.triangles.len();
            if v < 0. {
                for (triangle, edges) in mesh.triangles[start..end]
                    .iter_mut()
                    .zip(&mut adjacent[start..end])
                {
                    triangle.swap(1, 2);
                    edges.swap(0, 2);
                }
            }
            volume += v.abs();
            parts.push(start..end);
        }
        let eps = ray_offset(&mesh);
        let facets: Vec<_> = mesh.triangles.iter().map(|&t| geometry(&mesh, t)).collect();
        let area = facets.iter().map(|f| f.area).sum();
        Ok(Self {
            mesh,
            facets,
            adjacent,
            parts,
            volume,
            area,
            eps,
        })
    }
    pub fn region(
        &self,
        start: usize,
        seen: &mut [bool],
        mut belongs: impl FnMut(usize) -> bool,
    ) -> Vec<usize> {
        let mut group = vec![start];
        seen[start] = true;
        let mut at = 0;
        while at < group.len() {
            for j in self.adjacent[group[at]] {
                if j != NONE && !seen[j] && belongs(j) {
                    seen[j] = true;
                    group.push(j);
                }
            }
            at += 1;
        }
        group
    }
}
