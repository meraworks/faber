//! Spatial queries on a triangle mesh. Distances are in millimetres.

use faber_ports::{Mesh, Result};
use glam::DVec3;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f64,
    pub triangle: u32,
    pub point: [f64; 3],
}

struct Triangle {
    points: [DVec3; 3],
    source: u32,
}

impl Triangle {
    fn centre(&self, axis: usize) -> f64 {
        self.points.iter().map(|p| p[axis] / 3.).sum()
    }

    fn intersect(&self, ray: Ray) -> Option<(Hit, [f64; 3])> {
        let [a, b, c] = self.points;
        let ab = b - a;
        let ac = c - a;
        let h = ray.direction.cross(ac);
        let determinant = ab.dot(h);
        if determinant == 0. {
            return None;
        }
        let s = ray.origin - a;
        let u = s.dot(h) / determinant;
        let q = s.cross(ab);
        let v = ray.direction.dot(q) / determinant;
        // Include both sides of a shared edge. The query keeps a single hit.
        let epsilon = 8. * f64::EPSILON;
        if u < -epsilon || v < -epsilon || u + v > 1. + epsilon {
            return None;
        }
        let t = ac.dot(q) / determinant;
        (t >= 0. && t.is_finite()).then(|| {
            (
                Hit {
                    t,
                    triangle: self.source,
                    point: (ray.origin + t * ray.direction).into(),
                },
                [1. - u - v, u, v],
            )
        })
    }

    // Two endpoint keys identify an edge, including seams with separate vertex indices.
    fn boundary(&self, weights: [f64; 3]) -> Option<[[u64; 3]; 2]> {
        let on = weights.map(|w| w <= 8. * f64::EPSILON);
        let pair = match on {
            [false, true, true] => [0, 0],
            [true, false, true] => [1, 1],
            [true, true, false] => [2, 2],
            [true, false, false] => [1, 2],
            [false, true, false] => [0, 2],
            [false, false, true] => [0, 1],
            _ => return None,
        };
        let mut keys = pair.map(|i| {
            self.points[i]
                .to_array()
                .map(|n| if n == 0. { 0 } else { n.to_bits() })
        });
        keys.sort_unstable();
        Some(keys)
    }

    fn closest(&self, p: DVec3) -> DVec3 {
        let [a, b, c] = self.points;
        let ab = b - a;
        let ac = c - a;
        let n = ab.cross(ac);
        let projected = p - n * (p - a).dot(n) / n.length_squared();
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(l, r)| n.dot((r - l).cross(projected - l)) >= 0.)
        {
            return projected;
        }
        // The nearest point outside the face lies on one of its edges.
        [(a, b), (b, c), (c, a)]
            .map(|(l, r)| {
                let edge = r - l;
                l + edge * ((p - l).dot(edge) / edge.length_squared()).clamp(0., 1.)
            })
            .into_iter()
            .min_by(|l, r| l.distance_squared(p).total_cmp(&r.distance_squared(p)))
            .unwrap()
    }
}

#[derive(Clone, Copy)]
struct Ray {
    origin: DVec3,
    direction: DVec3,
}

impl Ray {
    fn new(origin: [f64; 3], direction: [f64; 3]) -> Option<Self> {
        let origin = DVec3::from(origin);
        let v = DVec3::from(direction);
        if !origin.is_finite() || !v.is_finite() {
            return None;
        }
        let direction = (v / v.abs().max_element()).try_normalize()?;
        Some(Self { origin, direction })
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    low: DVec3,
    high: DVec3,
}

impl Bounds {
    fn entry(self, ray: Ray, limit: f64) -> Option<f64> {
        let mut lo = 0_f64;
        let mut hi = limit;
        for axis in 0..3 {
            let d = ray.direction[axis];
            let o = ray.origin[axis];
            if d == 0. {
                if o < self.low[axis] || o > self.high[axis] {
                    return None;
                }
            } else {
                let a = (self.low[axis] - o) / d;
                let b = (self.high[axis] - o) / d;
                lo = lo.max(a.min(b));
                hi = hi.min(a.max(b));
                if lo > hi {
                    return None;
                }
            }
        }
        Some(lo)
    }

    fn distance_sq(self, p: DVec3) -> f64 {
        p.distance_squared(p.clamp(self.low, self.high))
    }
}

enum Children {
    Leaf(std::ops::Range<usize>),
    Pair(usize, usize),
}
struct Node {
    bounds: Bounds,
    children: Children,
}

/// Median splits on the longest axis. Leaves contain at most four triangles.
pub struct Bvh {
    nodes: Vec<Node>,
    triangles: Vec<Triangle>,
}

impl Bvh {
    pub fn new(mesh: &Mesh) -> Result<Self> {
        if mesh.triangles.is_empty() {
            return Err("The mesh has no triangle.".into());
        }
        if mesh.triangles.len() > u32::MAX as usize {
            return Err("The mesh has too many triangles.".into());
        }
        if mesh.vertices.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Mesh coordinates must be finite.".into());
        }
        let read = |(i, indices): (usize, &[u32; 3])| -> Result<Option<Triangle>> {
            if indices.iter().any(|&v| v as usize >= mesh.vertices.len()) {
                return Err(format!("Triangle {i} has a vertex outside the mesh."));
            }
            let points = indices.map(|v| DVec3::from(mesh.vertices[v as usize]));
            let [a, b, c] = points;
            let area = (b - a).cross(c - a).length_squared();
            if !area.is_finite() {
                return Err(format!("Triangle {i} exceeds the coordinate limit."));
            }
            Ok((area > 0.).then_some(Triangle {
                points,
                source: i as u32,
            }))
        };
        let entries = if mesh.triangles.len() >= 8192 {
            mesh.triangles
                .par_iter()
                .enumerate()
                .with_min_len(4096)
                .map(read)
                .collect::<Result<Vec<_>>>()?
        } else {
            mesh.triangles
                .iter()
                .enumerate()
                .map(read)
                .collect::<Result<Vec<_>>>()?
        };
        let mut triangles: Vec<_> = entries.into_iter().flatten().collect();
        if triangles.is_empty() {
            return Err("The mesh has no triangle with an area.".into());
        }
        let mut nodes = Vec::new();
        Self::split(&mut triangles, 0, &mut nodes);
        Ok(Self { nodes, triangles })
    }

    fn split(triangles: &mut [Triangle], start: usize, nodes: &mut Vec<Node>) -> usize {
        let mut bounds = Bounds {
            low: DVec3::INFINITY,
            high: DVec3::NEG_INFINITY,
        };
        for triangle in triangles.iter() {
            for point in triangle.points {
                bounds.low = bounds.low.min(point);
                bounds.high = bounds.high.max(point);
            }
        }
        let id = nodes.len();
        nodes.push(Node {
            bounds,
            children: Children::Leaf(start..start + triangles.len()),
        });
        if triangles.len() > 4 {
            let axis = (bounds.high - bounds.low).max_position();
            let middle = triangles.len() / 2;
            triangles.select_nth_unstable_by(middle, |a, b| {
                a.centre(axis)
                    .total_cmp(&b.centre(axis))
                    .then(a.source.cmp(&b.source))
            });
            let (l, r) = triangles.split_at_mut(middle);
            let l = Self::split(l, start, nodes);
            let r = Self::split(r, start + middle, nodes);
            nodes[id].children = Children::Pair(l, r);
        }
        id
    }

    fn visit_ray(
        &self,
        id: usize,
        ray: Ray,
        max: &mut f64,
        accept: &mut impl FnMut(Hit, &Triangle, [f64; 3], &mut f64),
    ) {
        match &self.nodes[id].children {
            Children::Leaf(range) => {
                for tri in &self.triangles[range.clone()] {
                    if let Some((hit, weights)) = tri.intersect(ray)
                        && hit.t <= *max
                    {
                        accept(hit, tri, weights, max);
                    }
                }
            }
            Children::Pair(l, r) => {
                let entries = [*l, *r].map(|id| {
                    (
                        self.nodes[id]
                            .bounds
                            .entry(ray, *max)
                            .unwrap_or(f64::INFINITY),
                        id,
                    )
                });
                let [a, b] = entries;
                let ordered = if a.0 <= b.0 { [a, b] } else { [b, a] };
                for (t, id) in ordered {
                    if t.is_finite() && t <= *max {
                        self.visit_ray(id, ray, max, accept);
                    }
                }
            }
        }
    }

    /// Find the nearest hit in the ray direction. The direction can have any length.
    pub fn first_hit(&self, origin: [f64; 3], direction: [f64; 3]) -> Option<Hit> {
        let ray = Ray::new(origin, direction)?;
        self.nodes[0].bounds.entry(ray, f64::INFINITY)?;
        let mut best: Option<Hit> = None;
        let mut max = f64::INFINITY;
        self.visit_ray(0, ray, &mut max, &mut |hit, _, _, max| {
            if best.is_none_or(|b| hit.t < b.t || (hit.t == b.t && hit.triangle < b.triangle)) {
                *max = hit.t;
                best = Some(hit);
            }
        });
        best
    }

    /// Return hits in distance order. A shared edge or vertex gives one hit.
    pub fn hits(&self, origin: [f64; 3], direction: [f64; 3]) -> Vec<Hit> {
        let mut hits = Vec::new();
        let mut boundary = std::collections::HashMap::new();
        if let Some(ray) = Ray::new(origin, direction)
            && self.nodes[0].bounds.entry(ray, f64::INFINITY).is_some()
        {
            let mut max = f64::INFINITY;
            self.visit_ray(0, ray, &mut max, &mut |hit, tri, weights, _| {
                if let Some(edge) = tri.boundary(weights) {
                    let first = boundary.entry(edge).or_insert(hit);
                    if hit.t < first.t || (hit.t == first.t && hit.triangle < first.triangle) {
                        *first = hit;
                    }
                } else {
                    hits.push(hit);
                }
            });
        }
        hits.extend(boundary.into_values());
        hits.sort_unstable_by(|a, b| a.t.total_cmp(&b.t).then(a.triangle.cmp(&b.triangle)));
        hits
    }

    /// Test a closed mesh. Points on its surface are outside.
    pub fn inside(&self, point: [f64; 3]) -> bool {
        let p = DVec3::from(point);
        let bounds = self.nodes[0].bounds;
        if !p.cmpgt(bounds.low).all() || !p.cmplt(bounds.high).all() {
            return false;
        }
        let extent = (bounds.high - bounds.low).max_element();
        if self.closest(point).t <= 4. * f64::EPSILON * extent {
            return false;
        }
        let a = 2_f64.sqrt();
        let b = 3_f64.sqrt();
        [[1., a, b], [-b, 1., a], [a, -b, 1.]]
            .iter()
            .filter(|&&d| self.hits(point, d).len() % 2 == 1)
            .count()
            >= 2
    }

    /// Find the nearest point on the mesh. The triangle index is from the input mesh.
    pub fn closest(&self, point: [f64; 3]) -> Hit {
        let p = DVec3::from(point);
        let mut best = Hit {
            t: f64::INFINITY,
            triangle: u32::MAX,
            point,
        };
        self.nearest(0, p, &mut best);
        best.t = best.t.sqrt();
        best
    }

    fn nearest(&self, id: usize, p: DVec3, best: &mut Hit) {
        match &self.nodes[id].children {
            Children::Leaf(range) => {
                for tri in &self.triangles[range.clone()] {
                    let point = tri.closest(p);
                    let t = p.distance_squared(point);
                    if t < best.t || (t == best.t && tri.source < best.triangle) {
                        *best = Hit {
                            t,
                            triangle: tri.source,
                            point: point.into(),
                        };
                    }
                }
            }
            Children::Pair(l, r) => {
                let [a, b] = [*l, *r].map(|id| (self.nodes[id].bounds.distance_sq(p), id));
                for (distance, id) in if a.0 <= b.0 { [a, b] } else { [b, a] } {
                    if distance <= best.t {
                        self.nearest(id, p, best);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bvh, Hit};
    use faber_ports::Mesh;
    use glam::DVec3;

    fn cube() -> Mesh {
        Mesh {
            vertices: vec![
                [0., 0., 0.],
                [10., 0., 0.],
                [10., 10., 0.],
                [0., 10., 0.],
                [0., 0., 10.],
                [10., 0., 10.],
                [10., 10., 10.],
                [0., 10., 10.],
            ],
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [1, 2, 6],
                [1, 6, 5],
                [2, 3, 7],
                [2, 7, 6],
                [3, 0, 4],
                [3, 4, 7],
            ],
            normals: vec![],
        }
    }

    #[test]
    fn first_hit_gives_the_distance() {
        let bvh = Bvh::new(&cube()).unwrap();
        let hit = bvh.first_hit([5., 5., -5.], [0., 0., 4.]).unwrap();
        assert!((hit.t - 5.).abs() < 1e-12);
        assert_eq!(hit.point, [5., 5., 0.]);
        assert_eq!(hit.triangle, 0);
        assert!(bvh.first_hit([5., 5., -5.], [0., 0., -1.]).is_none());
    }

    #[test]
    fn hits_are_sorted() {
        let bvh = Bvh::new(&cube()).unwrap();
        for origin in [[5., 5., -5.], [0., 0., -5.], [5., 0., -5.]] {
            let distances: Vec<_> = bvh.hits(origin, [0., 0., 1.]).iter().map(|h| h.t).collect();
            assert_eq!(distances, [5., 15.]);
        }
    }

    #[test]
    fn thin_faces_stay_separate_for_a_distant_ray() {
        let mut mesh = cube();
        for p in &mut mesh.vertices {
            p[2] *= 0.0001;
        }
        let hits = Bvh::new(&mesh).unwrap().hits([5., 5., -1e12], [0., 0., 1.]);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].t < hits[1].t);
    }

    #[test]
    fn inside_is_stable_on_edges_and_vertices() {
        let bvh = Bvh::new(&cube()).unwrap();
        let grid = [-1., 0., 0.125, 1., 2., 5., 8., 9.875, 10., 11.];
        for i in 0..1000 {
            let n = (713 * i + 137) % 1000;
            let point = [grid[n % 10], grid[n / 10 % 10], grid[n / 100]];
            assert_eq!(
                bvh.inside(point),
                point.iter().all(|x| 0. < *x && *x < 10.),
                "{point:?}"
            );
        }
    }

    #[test]
    fn closest_gives_the_face_point() {
        let bvh = Bvh::new(&cube()).unwrap();
        let hit = bvh.closest([5., 5., 14.]);
        assert_eq!(hit.point, [5., 5., 10.]);
        assert_eq!(hit.t, 4.);
        assert_eq!(bvh.closest([11., 11., 11.]).point, [10., 10., 10.]);
    }

    #[test]
    fn empty_mesh_is_refused() {
        let mut mesh = cube();
        mesh.triangles.clear();
        assert_eq!(
            Bvh::new(&mesh).err().as_deref(),
            Some("The mesh has no triangle.")
        );
    }

    #[test]
    fn invalid_triangles_are_refused() {
        let mut mesh = cube();
        mesh.triangles[0][2] = 99;
        assert!(Bvh::new(&mesh).is_err());
        mesh.triangles = vec![[0, 0, 0]];
        assert!(Bvh::new(&mesh).is_err());
        mesh = cube();
        mesh.vertices[0][0] = f64::NAN;
        assert!(Bvh::new(&mesh).is_err());
    }

    struct Seed(u64);
    impl Seed {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn sphere(rings: usize, sides: usize) -> Mesh {
        let mut random = Seed(712761);
        let mut vertices = vec![[0., 0., 1.]];
        let mut triangles = Vec::new();
        for r in 0..rings {
            let phi = std::f64::consts::PI * (r + 1) as f64 / (rings + 1) as f64;
            let start = random.next() * 0.4;
            for s in 0..sides {
                let theta = std::f64::consts::TAU * (s as f64 + start) / sides as f64;
                vertices.push([theta.cos() * phi.sin(), theta.sin() * phi.sin(), phi.cos()]);
            }
        }
        let bottom = vertices.len() as u32;
        vertices.push([0., 0., -1.]);
        let vertex = |r: usize, s: usize| (1 + r * sides + s % sides) as u32;
        for s in 0..sides {
            triangles.push([0, vertex(0, s), vertex(0, s + 1)]);
            for r in 1..rings {
                let [a, b, c, d] = [
                    vertex(r - 1, s),
                    vertex(r, s),
                    vertex(r, s + 1),
                    vertex(r - 1, s + 1),
                ];
                triangles.extend([[a, b, c], [a, c, d]]);
            }
            triangles.push([vertex(rings - 1, s), bottom, vertex(rings - 1, s + 1)]);
        }
        Mesh {
            vertices,
            triangles,
            normals: vec![],
        }
    }

    fn ray(seed: &mut Seed) -> (DVec3, DVec3) {
        let origin = DVec3::new(3., seed.next() * 4. - 2., seed.next() * 4. - 2.);
        let target = DVec3::new(0., seed.next() * 2.6 - 1.3, seed.next() * 2.6 - 1.3);
        (origin, (target - origin).normalize())
    }

    // Intersect the plane, then check its three half spaces.
    fn brute(mesh: &Mesh, origin: DVec3, direction: DVec3) -> Option<(f64, u32)> {
        mesh.triangles
            .iter()
            .enumerate()
            .filter_map(|(i, tri)| {
                let [a, b, c] = tri.map(|v| DVec3::from(mesh.vertices[v as usize]));
                let normal = (b - a).cross(c - a);
                let t = normal.dot(a - origin) / normal.dot(direction);
                if !t.is_finite() || t < 0. {
                    return None;
                }
                let p = origin + t * direction;
                [(a, b), (b, c), (c, a)]
                    .into_iter()
                    .all(|(a, b)| normal.dot((b - a).cross(p - a)) >= 0.)
                    .then_some((t, i as u32))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    #[test]
    fn matches_brute_force() {
        let mesh = sphere(50, 50);
        assert_eq!(mesh.triangles.len(), 5000);
        let bvh = Bvh::new(&mesh).unwrap();
        let mut seed = Seed(1241);
        for _ in 0..500 {
            let (o, d) = ray(&mut seed);
            let hit = bvh.first_hit(o.into(), d.into());
            let reference = brute(&mesh, o, d);
            assert_eq!(hit.as_ref().map(|h| h.triangle), reference.map(|h| h.1));
            if let (Some(Hit { t, .. }), Some((expected, _))) = (hit, reference) {
                assert!((t - expected).abs() < 1e-12);
            }
        }
    }

    #[test]
    #[ignore = "Run with the release profile to measure ray queries."]
    fn throughput() {
        let mesh = sphere(100, 500);
        assert_eq!(mesh.triangles.len(), 100_000);
        let bvh = Bvh::new(&mesh).unwrap();
        let mut seed = Seed(1241);
        let rays: Vec<_> = (0..100_000).map(|_| ray(&mut seed)).collect();
        let start = std::time::Instant::now();
        let count = rays
            .into_iter()
            .filter(|(o, d)| bvh.first_hit((*o).into(), (*d).into()).is_some())
            .count();
        println!(
            "100000 first-hit rays, 100000 triangles, {} ms, {count} hits",
            start.elapsed().as_millis()
        );
        assert!((40_000..90_000).contains(&count));
    }
}
