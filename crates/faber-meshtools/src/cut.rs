use faber_ports::{Mesh, Result};
use glam::DVec3;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Above,
    Below,
    Both,
}

const EMPTY: &str = "The plane does not cut the mesh. The selected side is empty.";

struct Input {
    points: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    normal: DVec3,
    distance: Vec<f64>,
}

impl Input {
    fn new(mesh: &Mesh, origin: [f64; 3], normal: [f64; 3]) -> Result<Self> {
        let origin = DVec3::from(origin);
        let normal = DVec3::from(normal)
            .try_normalize()
            .filter(|n| n.is_finite() && n.length_squared() > 0.5)
            .ok_or("The cut needs a finite, non-zero plane normal.")?;
        if !origin.is_finite() {
            return Err("The cut plane origin must be finite.".into());
        }
        let (vertices, map) = mesh.welded_vertices(1e-8)?;
        let mut points: Vec<_> = vertices.into_iter().map(DVec3::from).collect();
        let mut distance = Vec::with_capacity(points.len());
        for p in &mut points {
            let relative = *p - origin;
            let d = relative.dot(normal);
            if !d.is_finite() {
                return Err("The mesh exceeds the cut plane range.".into());
            }
            if d.abs() <= 1e-9 + 16. * f64::EPSILON * relative.abs().max_element() {
                *p -= normal * d;
                distance.push(0.);
            } else {
                distance.push(d);
            }
        }
        Ok(Self {
            points,
            triangles: mesh
                .triangles
                .iter()
                .map(|t| t.map(|i| map[i as usize]))
                .collect(),
            normal,
            distance,
        })
    }

    fn clipped(&self, sign: f64, include_coplanar: bool) -> Side {
        let mut side = Side::default();
        let mut indices = HashMap::new();
        for &tri in &self.triangles {
            let d = tri.map(|i| sign * self.distance[i as usize]);
            if !include_coplanar && d.iter().all(|v| *v < 0.) {
                continue;
            }
            if d.iter().all(|v| *v >= 0.) {
                if !include_coplanar || d != [0.; 3] {
                    continue;
                }
                // A coincident face belongs to the side that contains its material.
                let [a, b, c] = tri.map(|id| self.points[id as usize]);
                if (b - a).cross(c - a).dot(self.normal) * sign <= 0. {
                    continue;
                }
            }
            // An original vertex uses (i, i). Both faces of a cut edge use the same key.
            let mut polygon = [0u32; 4];
            let mut count = 0;
            for i in 0..3 {
                let j = (i + 1) % 3;
                let (a, b) = (tri[i], tri[j]);
                if d[i] <= 0. {
                    polygon[count] = *indices
                        .entry((a, a))
                        .or_insert_with(|| side.vertex(self.points[a as usize], d[i] == 0.));
                    count += 1;
                }
                if (d[i] < 0. && d[j] > 0.) || (d[j] < 0. && d[i] > 0.) {
                    polygon[count] = *indices.entry((a.min(b), a.max(b))).or_insert_with(|| {
                        let p = self.points[a as usize]
                            .lerp(self.points[b as usize], d[i] / (d[i] - d[j]));
                        side.vertex(p, true)
                    });
                    count += 1;
                }
            }
            for j in 2..count {
                side.triangle([polygon[0], polygon[j - 1], polygon[j]]);
            }
        }
        side
    }
}

#[derive(Default)]
struct Side {
    mesh: Mesh,
    on_plane: Vec<bool>,
    boundary: BTreeSet<(u32, u32)>,
}

impl Side {
    fn vertex(&mut self, p: DVec3, on_plane: bool) -> u32 {
        let id = self.mesh.vertices.len() as u32;
        self.mesh.vertices.push(p.to_array());
        self.on_plane.push(on_plane);
        id
    }

    fn triangle(&mut self, triangle: [u32; 3]) {
        let [a, b, c] = triangle.map(|i| DVec3::from(self.mesh.vertices[i as usize]));
        if (b - a).cross(c - a).length_squared() == 0. {
            return;
        }
        self.mesh.triangles.push(triangle);
        for i in 0..3 {
            let (a, b) = (triangle[i], triangle[(i + 1) % 3]);
            if self.on_plane[a as usize] && self.on_plane[b as usize] {
                let pair = (a.min(b), a.max(b));
                if !self.boundary.remove(&pair) {
                    self.boundary.insert(pair);
                }
            }
        }
    }

    fn loops(&self) -> Result<Vec<Vec<u32>>> {
        let mut adjacent: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(a, b) in &self.boundary {
            adjacent.entry(a).or_default().push(b);
            adjacent.entry(b).or_default().push(a);
        }
        if adjacent.values().any(|next| next.len() != 2) {
            return Err("The cut boundary branches or is open. Repair the mesh first.".into());
        }
        let mut rings = Vec::new();
        while let Some((&start, _)) = adjacent.first_key_value() {
            let mut ring = vec![start];
            let (mut previous, mut at) = (start, adjacent.remove(&start).unwrap()[0]);
            while at != start {
                let next = adjacent
                    .remove(&at)
                    .ok_or("The cut boundary overlaps itself.")?;
                ring.push(at);
                (previous, at) = (
                    at,
                    if next[0] == previous {
                        next[1]
                    } else {
                        next[0]
                    },
                );
            }
            rings.push(ring);
        }
        Ok(rings)
    }

    fn cap(&mut self, n: DVec3) -> Result<()> {
        if self.boundary.is_empty() {
            return Ok(());
        }
        let triangles = cap_loops(&self.mesh, &self.loops()?, n)?;
        self.mesh.triangles.extend(triangles);
        Ok(())
    }
}

/// Triangulate planar contours with holes. Keep every boundary edge in the mesh.
pub(crate) fn cap_loops(mesh: &Mesh, rings: &[Vec<u32>], n: DVec3) -> Result<Vec<[u32; 3]>> {
    if rings.is_empty() {
        return Ok(Vec::new());
    }
    if rings.iter().any(|ring| ring.len() < 3) {
        return Err("A boundary needs at least three vertices.".into());
    }
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let mut ids = Vec::new();
    let mut edges = Vec::new();
    for ring in rings {
        let base = ids.len();
        for i in 0..ring.len() {
            edges.push([base + i, base + (i + 1) % ring.len()]);
        }
        ids.extend_from_slice(ring);
    }
    let origin = DVec3::from(mesh.vertices[ids[0] as usize]);
    let points = ids
        .iter()
        .map(|&id| {
            let p = DVec3::from(mesh.vertices[id as usize]) - origin;
            Point2::new(p.dot(u), p.dot(v))
        })
        .collect();
    let mut conflict = false;
    let tess =
        ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(points, edges, |_| {
            conflict = true
        })
        .map_err(|e| format!("Cannot fill the cut boundary: {e}"))?;
    if conflict || tess.num_vertices() != ids.len() || tess.num_constraints() != ids.len() {
        return Err("The cut has intersecting boundary edges.".into());
    }
    // Start outside the mesh. Each boundary crossing changes which faces contain material.
    let mut adjacent = vec![Vec::new(); tess.num_all_faces()];
    for edge in tess.undirected_edges() {
        let directed = edge.as_directed();
        let a = directed.face().index();
        let b = directed.rev().face().index();
        adjacent[a].push((b, edge.is_constraint_edge()));
        adjacent[b].push((a, edge.is_constraint_edge()));
    }
    let outside = tess.outer_face().index();
    let mut filled = vec![None; adjacent.len()];
    filled[outside] = Some(false);
    let mut queue = VecDeque::from([outside]);
    while let Some(face) = queue.pop_front() {
        for &(next, crosses) in &adjacent[face] {
            let inside = filled[face].unwrap() ^ crosses;
            if let Some(previous) = filled[next] {
                if previous != inside {
                    return Err("The cut has inconsistent boundaries.".into());
                }
            } else {
                filled[next] = Some(inside);
                queue.push_back(next);
            }
        }
    }
    Ok(tess
        .inner_faces()
        .filter(|f| filled[f.index()] == Some(true))
        .map(|f| f.vertices().map(|v| ids[v.index()]))
        .collect())
}

/// Return closed contours. Each contour repeats its first point at the end.
pub fn slice(mesh: &Mesh, origin: [f64; 3], normal: [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    let Ok(input) = Input::new(mesh, origin, normal) else {
        return Vec::new();
    };
    let sign = if input.distance.iter().any(|d| *d < 0.) {
        1.
    } else {
        -1.
    };
    let side = input.clipped(sign, false);
    side.loops()
        .unwrap_or_default()
        .into_iter()
        .map(|ring| {
            ring.iter()
                .chain(ring.first())
                .map(|&id| side.mesh.vertices[id as usize])
                .collect()
        })
        .collect()
}

/// Keep either side of the plane. A two-part result has the lower side first.
pub fn cut(
    mesh: &Mesh,
    origin: [f64; 3],
    normal: [f64; 3],
    keep: Keep,
    cap: bool,
) -> Result<Vec<Mesh>> {
    let input = Input::new(mesh, origin, normal)?;
    let below = input.distance.iter().any(|d| *d < 0.);
    let above = input.distance.iter().any(|d| *d > 0.);
    if !(above && below) {
        if mesh.triangles.is_empty()
            || (above && keep == Keep::Below)
            || (below && keep == Keep::Above)
        {
            return Err(EMPTY.into());
        }
        return Ok(vec![mesh.clone()]);
    }
    let signs: &[f64] = match keep {
        Keep::Below => &[1.],
        Keep::Above => &[-1.],
        Keep::Both => &[1., -1.],
    };
    signs
        .iter()
        .map(|&s| {
            let mut side = input.clipped(s, true);
            if cap {
                side.cap(input.normal * s)?;
            }
            Ok(side.mesh)
        })
        .collect()
}
