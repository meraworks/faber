//! Quadric error edge collapse on a closed triangle surface.
use crate::topology::{self, Topology};
use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use glam::{DMat3, DVec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap, HashSet};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Triangles(usize),
    MaxError(f64),
    Ratio(f64),
}
#[derive(Clone, Debug, Serialize)]
pub struct ReduceLog {
    pub before: usize,
    pub after: usize,
    pub max_error_mm: f64,
    pub changed: bool,
    pub message: String,
}

#[derive(Clone, Copy)]
struct Quadric {
    a: DMat3,
    b: DVec3,
    c: f64,
}
impl Quadric {
    const ZERO: Self = Self {
        a: DMat3::ZERO,
        b: DVec3::ZERO,
        c: 0.,
    };
    fn plane(n: DVec3, p: DVec3) -> Self {
        let d = -n.dot(p);
        Self {
            a: DMat3::from_cols(n * n.x, n * n.y, n * n.z),
            b: d * n,
            c: d * d,
        }
    }
    fn sum(self, rhs: Self) -> Self {
        Self {
            a: self.a + rhs.a,
            b: self.b + rhs.b,
            c: self.c + rhs.c,
        }
    }
    fn cost(self, p: DVec3) -> f64 {
        (p.dot(self.a * p) + 2. * self.b.dot(p) + self.c).max(0.)
    }
    fn minimum(self) -> Option<DVec3> {
        let scale = self.a.x_axis.x.max(self.a.y_axis.y).max(self.a.z_axis.z);
        if self.a.determinant().abs() <= 1e-12 * scale.powi(3) {
            return None;
        }
        let p = -(self.a.inverse() * self.b);
        p.is_finite().then_some(p)
    }
}
#[derive(Clone, Copy)]
enum Feature {
    Surface,
    Crease(DVec3),
    Corner,
}
#[derive(Clone)]
struct Vertex {
    p: DVec3,
    q: Quadric,
    faces: BTreeSet<usize>,
    version: u32,
    feature: Feature,
}
#[derive(Clone, Copy)]
struct Choice {
    a: u32,
    b: u32,
    versions: [u32; 2],
    p: DVec3,
    cost: f64,
    length: f64,
}
impl Ord for Choice {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.length.total_cmp(&self.length))
            .then(other.a.cmp(&self.a))
            .then(other.b.cmp(&self.b))
    }
}
impl PartialOrd for Choice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Choice {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Choice {}

struct Surface {
    vertices: Vec<Vertex>,
    faces: Vec<Option<[u32; 3]>>,
    heap: BinaryHeap<Choice>,
    count: usize,
    origin: DVec3,
}
impl Surface {
    fn new(mesh: &Mesh) -> Self {
        let origin = DVec3::from(mesh.vertices[0]);
        let mut vertices: Vec<_> = mesh
            .vertices
            .iter()
            .map(|&p| Vertex {
                p: DVec3::from(p) - origin,
                q: Quadric::ZERO,
                faces: BTreeSet::new(),
                version: 0,
                feature: Feature::Surface,
            })
            .collect();
        let mut normals = Vec::with_capacity(mesh.triangles.len());
        for (i, &t) in mesh.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|v| vertices[v as usize].p);
            let n = (b - a).cross(c - a).normalize();
            normals.push(n);
            let q = Quadric::plane(n, a);
            for id in t {
                let v = &mut vertices[id as usize];
                v.q = v.q.sum(q);
                v.faces.insert(i);
            }
        }
        let graph = Topology::new(mesh);
        let mut sharp: Vec<Vec<DVec3>> = vec![Vec::new(); vertices.len()];
        for (&(a, b), incidents) in &graph.edges {
            let [(left, _), (right, _)] = incidents.as_slice() else {
                continue;
            };
            if normals[*left].dot(normals[*right]) < 35_f64.to_radians().cos() {
                let along = (vertices[a as usize].p - vertices[b as usize].p).normalize();
                sharp[a as usize].push(along);
                sharp[b as usize].push(along);
            }
        }
        for (v, edges) in vertices.iter_mut().zip(sharp) {
            v.feature = match edges.as_slice() {
                [] => Feature::Surface,
                [a, b] if a.dot(*b).abs() > 1. - 1e-8 => Feature::Crease(*a),
                _ => Feature::Corner,
            };
        }
        let mut s = Self {
            vertices,
            faces: mesh.triangles.iter().copied().map(Some).collect(),
            count: mesh.triangles.len(),
            heap: BinaryHeap::new(),
            origin,
        };
        for &(a, b) in graph.edges.keys() {
            s.queue(a, b);
        }
        s
    }
    fn queue(&mut self, i: u32, j: u32) {
        let (a, b) = topology::key(i, j);
        let va = &self.vertices[a as usize];
        let vb = &self.vertices[b as usize];
        let line = vb.p - va.p;
        let length = line.length_squared();
        let mid = (va.p + vb.p) * 0.5;
        let q = va.q.sum(vb.q);
        let choose = |points: &[DVec3]| {
            points
                .iter()
                .copied()
                .min_by(|u, v| q.cost(*u).total_cmp(&q.cost(*v)))
                .unwrap_or(mid)
        };
        let p = match (va.feature, vb.feature) {
            (Feature::Corner, Feature::Corner) => return,
            (Feature::Corner, Feature::Crease(n))
                if n.cross(line).length_squared() > length * 1e-12 =>
            {
                return;
            }
            (Feature::Crease(n), Feature::Corner)
                if n.cross(line).length_squared() > length * 1e-12 =>
            {
                return;
            }
            (Feature::Corner, _) | (Feature::Crease(_), Feature::Surface) => va.p,
            (_, Feature::Corner) | (Feature::Surface, Feature::Crease(_)) => vb.p,
            (Feature::Crease(n), Feature::Crease(m)) => {
                if n.dot(m).abs() < 1. - 1e-8 || n.cross(line).length_squared() > length * 1e-12 {
                    return;
                }
                choose(&[mid, va.p, vb.p])
            }
            (Feature::Surface, Feature::Surface) => {
                let minimum = q
                    .minimum()
                    .filter(|p| p.distance_squared(mid) <= length * 2.)
                    .unwrap_or(mid);
                choose(&[minimum, mid, va.p, vb.p])
            }
        };
        self.heap.push(Choice {
            a,
            b,
            p,
            cost: q.cost(p),
            length,
            versions: [va.version, vb.version],
        });
    }
    fn neighbours(&self, i: u32) -> BTreeSet<u32> {
        self.vertices[i as usize]
            .faces
            .iter()
            .flat_map(|&f| self.faces[f])
            .flatten()
            .filter(|&v| v != i)
            .collect()
    }
    fn collapse(&mut self, c: Choice) -> bool {
        let (a, b) = (c.a, c.b);
        let (va, vb) = (&self.vertices[a as usize], &self.vertices[b as usize]);
        if [va.version, vb.version] != c.versions || va.faces.is_empty() || vb.faces.is_empty() {
            return false;
        }
        if va.faces.intersection(&vb.faces).count() != 2 {
            return false;
        }
        let na = self.neighbours(a);
        let nb = self.neighbours(b);
        if na.intersection(&nb).count() != 2 {
            return false;
        }
        let affected: Vec<_> = va.faces.union(&vb.faces).copied().collect();
        let mut keys = HashSet::new();
        for &i in &affected {
            let t = self.faces[i].unwrap();
            if t.contains(&a) && t.contains(&b) {
                continue;
            }
            let [o, p, q] = t.map(|v| self.vertices[v as usize].p);
            let before = (p - o).cross(q - o);
            let [o, p, q] = t.map(|v| {
                if v == a || v == b {
                    c.p
                } else {
                    self.vertices[v as usize].p
                }
            });
            let after = (p - o).cross(q - o);
            if after.length_squared() <= before.length_squared() * 1e-20 || before.dot(after) <= 0.
            {
                return false;
            }
            let mut key = t.map(|v| if v == b { a } else { v });
            key.sort_unstable();
            if !keys.insert(key) {
                return false;
            }
        }
        let feature = match (va.feature, vb.feature) {
            (Feature::Corner, _) | (_, Feature::Corner) => Feature::Corner,
            (Feature::Crease(n), _) | (_, Feature::Crease(n)) => Feature::Crease(n),
            _ => Feature::Surface,
        };
        let q = va.q.sum(vb.q);
        for &i in &affected {
            let t = self.faces[i].unwrap();
            if t.contains(&a) && t.contains(&b) {
                self.faces[i] = None;
                self.count -= 1;
                for v in t {
                    self.vertices[v as usize].faces.remove(&i);
                }
            } else if t.contains(&b) {
                self.faces[i] = Some(t.map(|v| if v == b { a } else { v }));
                self.vertices[a as usize].faces.insert(i);
            }
        }
        self.vertices[b as usize].faces.clear();
        self.vertices[b as usize].version += 1;
        let v = &mut self.vertices[a as usize];
        v.p = c.p;
        v.q = q;
        v.feature = feature;
        v.version += 1;
        for j in self.neighbours(a) {
            self.queue(a, j);
        }
        true
    }
    fn simplify(mut self, goal: usize, limit: f64) -> Mesh {
        while self.count > goal {
            let Some(c) = self.heap.pop() else { break };
            if c.cost > limit {
                break;
            }
            self.collapse(c);
        }
        let mut mesh = Mesh {
            vertices: self
                .vertices
                .into_iter()
                .map(|v| (v.p + self.origin).into())
                .collect(),
            triangles: self.faces.into_iter().flatten().collect(),
            normals: Vec::new(),
        };
        topology::compact(&mut mesh);
        mesh
    }
}

fn deviation(original: &Mesh, result: &Mesh) -> Result<f64> {
    let tree = Bvh::new(result)?;
    Ok(original
        .vertices
        .par_iter()
        .with_min_len(256)
        .map(|&p| tree.closest(p).t)
        .reduce(|| 0., f64::max))
}
pub fn reduce(input: &Mesh, target: Target) -> Result<(Mesh, ReduceLog)> {
    const TRIANGLE_LIMIT: usize = 2_000_000;
    let count = input.triangles.len();
    if count > TRIANGLE_LIMIT {
        return Err(format!(
            "The mesh has {count} triangles. The reduction limit is {TRIANGLE_LIMIT}. Divide the mesh first."
        ));
    }
    let original = topology::try_weld(input, topology::WELD_MM)?.mesh;
    let assessment = crate::report::inspected(&original)?;
    if !assessment.watertight {
        return Err(format!(
            "Reduction needs a closed, consistent mesh. Repair it first. {}",
            assessment.problems.join(", ")
        ));
    }
    let before = input.triangles.len();
    let (goal,max_error)=match target{
        Target::Triangles(n) if n>0=>(n,None),
        Target::Ratio(r) if r.is_finite()&&r>0.&&r<=1.=>(((before as f64*r).ceil() as usize).max(1),None),
        Target::MaxError(e) if e.is_finite()&&e>=0.=>(4,Some(e)),
        _=>return Err("Use a positive triangle count, a ratio above zero and at most one, or a finite nonnegative error.".into()),
    };
    let unchanged = || {
        (
            input.clone(),
            ReduceLog {
                before,
                after: before,
                changed: false,
                max_error_mm: 0.,
                message: "The mesh is unchanged.".into(),
            },
        )
    };
    if goal >= before {
        return Ok(unchanged());
    }
    let limit = max_error.map_or(f64::INFINITY, |e| e * e);
    let mut aim = goal.max(4);
    loop {
        let result = Surface::new(&original).simplify(aim, limit);
        let after = result.triangles.len();
        if after >= before {
            return Ok(unchanged());
        }
        let error = deviation(&original, &result)?;
        if max_error.is_none_or(|e| error <= e) {
            let message = if after > goal && max_error.is_none() {
                "Further reduction would change the mesh topology or a sharp feature."
            } else {
                "The mesh was reduced."
            };
            return Ok((
                result,
                ReduceLog {
                    before,
                    after,
                    max_error_mm: error,
                    changed: true,
                    message: message.into(),
                },
            ));
        }
        // A quadric estimates error. Certify the result against the original
        // vertices and keep more triangles if the measured bound is too large.
        aim = after + (before - after).div_ceil(2);
        if aim >= before {
            return Ok(unchanged());
        }
    }
}
