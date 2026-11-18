//! Camera and section geometry shared by both views.

use faber_ports::{Mesh, Result};
use glam::{DVec3, Mat4, Vec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod rectangle;
pub use rectangle::{RectMode, mesh_in_rect};

/// Display normals per triangle corner.
pub fn corner_normals(mesh: &Mesh) -> Vec<[[f32; 3]; 3]> {
    if !mesh.normals.is_empty() && mesh.normals.len() == mesh.vertices.len() {
        return mesh
            .triangles
            .iter()
            .map(|t| {
                t.map(|i| {
                    DVec3::from(mesh.normals[i as usize])
                        .normalize_or_zero()
                        .as_vec3()
                        .to_array()
                })
            })
            .collect();
    }
    use std::collections::HashMap;
    let key = |p: [f64; 3]| p.map(|v| (v * 1e7).round() as i64);
    let mut adjacent: HashMap<[i64; 3], Vec<(DVec3, f64)>> = HashMap::new();
    let mut normals = Vec::with_capacity(mesh.triangles.len());
    for triangle in &mesh.triangles {
        let points = triangle.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        normals.push(normal);
        for corner in 0..3 {
            let a = (points[(corner + 1) % 3] - points[corner]).normalize_or_zero();
            let b = (points[(corner + 2) % 3] - points[corner]).normalize_or_zero();
            let angle = a.dot(b).clamp(-1., 1.).acos();
            adjacent
                .entry(key(points[corner].to_array()))
                .or_default()
                .push((normal, angle));
        }
    }
    let crease = 35f64.to_radians().cos();
    mesh.triangles
        .iter()
        .zip(normals)
        .map(|(triangle, normal)| {
            triangle.map(|i| {
                adjacent[&key(mesh.vertices[i as usize])]
                    .iter()
                    .filter(|(n, _)| n.dot(normal) >= crease)
                    .map(|(n, weight)| *n * *weight)
                    .sum::<DVec3>()
                    .normalize_or_zero()
                    .as_vec3()
                    .to_array()
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub normal: [f64; 3],
    pub offset: f64,
    #[serde(default)]
    pub reverse: bool,
}

impl Section {
    /// Express a world plane in the local frame of a rigid body.
    pub fn in_body(self, transform: glam::DMat4) -> Self {
        let n = DVec3::from(self.normal).normalize();
        Self {
            normal: transform.inverse().transform_vector3(n).to_array(),
            offset: self.offset - n.dot(transform.w_axis.truncate()),
            reverse: self.reverse,
        }
    }
}

pub struct Cut {
    pub surface: Mesh,
    pub cap: Mesh,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Iso,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Edge([i8; 3]),
    Corner([i8; 3]),
}

pub fn section(mesh: &Mesh, plane: Section) -> Result<Cut> {
    section_mesh(mesh, plane, true, mesh.triangles.iter())
}

pub fn section_cap(mesh: &Mesh, plane: Section) -> Result<Mesh> {
    Ok(section_mesh(mesh, plane, false, mesh.triangles.iter())?.cap)
}

/// Bounds for consecutive groups of triangles in one mesh.
pub struct SectionIndex(Vec<(DVec3, DVec3)>);

impl SectionIndex {
    const GROUP: usize = 64;

    pub fn new(mesh: &Mesh) -> Self {
        Self(
            mesh.triangles
                .chunks(Self::GROUP)
                .map(|group| {
                    let mut lo = DVec3::splat(f64::INFINITY);
                    let mut hi = -lo;
                    for triangle in group {
                        for &index in triangle {
                            let point = DVec3::from(mesh.vertices[index as usize]);
                            lo = lo.min(point);
                            hi = hi.max(point);
                        }
                    }
                    ((lo + hi) * 0.5, (hi - lo) * 0.5)
                })
                .collect(),
        )
    }

    pub fn cap(&self, mesh: &Mesh, plane: Section) -> Result<Mesh> {
        let n = DVec3::from(plane.normal).normalize();
        let triangles = self
            .0
            .iter()
            .zip(mesh.triangles.chunks(Self::GROUP))
            .filter(|((centre, half), _)| {
                (n.dot(*centre) - plane.offset).abs() <= n.abs().dot(*half) + 1e-8
            })
            .flat_map(|(_, group)| group);
        Ok(section_mesh(mesh, plane, false, triangles)?.cap)
    }
}

fn section_mesh<'a>(
    mesh: &Mesh,
    plane: Section,
    surface: bool,
    triangles: impl Iterator<Item = &'a [u32; 3]>,
) -> Result<Cut> {
    let mut n = DVec3::from(plane.normal);
    if !n.is_finite() || !n.length().is_finite() || n.length() < 1e-12 || !plane.offset.is_finite()
    {
        return Err("Section needs a finite normal and offset".into());
    }
    n = n.normalize();
    let mut offset = plane.offset;
    if plane.reverse {
        n = -n;
        offset = -offset;
    }
    let mut cut = Cut {
        surface: Mesh::default(),
        cap: Mesh::default(),
    };
    let mut segments = Vec::new();
    let has_normals = mesh.normals.len() == mesh.vertices.len();
    for t in triangles {
        let points = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let normals = if surface && has_normals {
            t.map(|i| DVec3::from(mesh.normals[i as usize]))
        } else {
            [DVec3::ZERO; 3]
        };
        let distances = points.map(|p| n.dot(p) - offset);
        if !surface
            && (distances.iter().all(|d| *d < -1e-8) || distances.iter().all(|d| *d >= -1e-8))
        {
            continue;
        }
        let mut polygon = Vec::new();
        let mut polygon_normals = Vec::new();
        let mut crossings = Vec::new();
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (a, b, da, db) = (points[i], points[j], distances[i], distances[j]);
            if surface && da <= 1e-8 {
                polygon.push(a);
                polygon_normals.push(normals[i]);
            }
            if (da < -1e-8 && db > 1e-8) || (da > 1e-8 && db < -1e-8) {
                let p = a + (b - a) * (da / (da - db));
                if surface {
                    polygon.push(p);
                    polygon_normals.push(
                        normals[i]
                            .lerp(normals[j], da / (da - db))
                            .normalize_or_zero(),
                    );
                }
                crossings.push(p);
            }
            if da.abs() <= 1e-8 {
                crossings.push(a);
            }
        }
        if crossings.len() == 2 && distances.iter().any(|d| *d < -1e-8) {
            segments.push((crossings[0], crossings[1]));
        }
        if polygon.len() >= 3 {
            let start = cut.surface.vertices.len() as u32;
            cut.surface
                .vertices
                .extend(polygon.iter().map(|p| p.to_array()));
            if has_normals {
                cut.surface
                    .normals
                    .extend(polygon_normals.iter().map(|n| n.to_array()));
            }
            for i in 1..polygon.len() - 1 {
                cut.surface
                    .triangles
                    .push([start, start + i as u32, start + i as u32 + 1]);
            }
        }
    }
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let key = |p: DVec3| p.to_array().map(|a| (a * 1e7).round() as i64);
    let mut nodes = BTreeMap::new();
    let mut positions = Vec::new();
    let mut edges = BTreeSet::new();
    for (a, b) in segments {
        let mut ids = [0; 2];
        for (i, p) in [a, b].into_iter().enumerate() {
            ids[i] = *nodes.entry(key(p)).or_insert_with(|| {
                positions.push(p);
                positions.len() - 1
            });
        }
        if ids[0] != ids[1] {
            let edge = (ids[0].min(ids[1]), ids[0].max(ids[1]));
            // Two equal segments are inside the retained surface.
            if !edges.insert(edge) {
                edges.remove(&edge);
            }
        }
    }
    // Index both directions so a contour step looks up only the next vertex.
    let mut neighbours = BTreeSet::new();
    for &(a, b) in &edges {
        neighbours.insert((a, b));
        neighbours.insert((b, a));
    }
    let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
    while let Some(&(a, b)) = neighbours.first() {
        neighbours.remove(&(a, b));
        neighbours.remove(&(b, a));
        let mut path = vec![a, b];
        let mut at = b;
        while at != a {
            let &(_, next) = neighbours
                .range((at, 0)..=(at, usize::MAX))
                .next()
                .ok_or("Section boundary is open")?;
            neighbours.remove(&(at, next));
            neighbours.remove(&(next, at));
            at = next;
            if at != a {
                path.push(at);
            }
        }
        if path.len() >= 3 {
            loops.push(
                path.iter()
                    .map(|i| [positions[*i].dot(u), positions[*i].dot(v)])
                    .collect(),
            );
        }
    }
    let inside = |p: [f64; 2], ring: &[[f64; 2]]| {
        let mut hit = false;
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                hit = !hit;
            }
        }
        hit
    };
    let depth: Vec<usize> = loops
        .iter()
        .enumerate()
        .map(|(i, r)| {
            loops
                .iter()
                .enumerate()
                .filter(|(j, s)| i != *j && inside(r[0], s))
                .count()
        })
        .collect();
    for (i, ring) in loops.iter().enumerate() {
        if !depth[i].is_multiple_of(2) {
            continue;
        }
        let mut flat: Vec<f64> = ring.iter().flatten().copied().collect();
        let mut holes = Vec::new();
        for (j, hole) in loops.iter().enumerate() {
            if depth[j] == depth[i] + 1 && inside(hole[0], ring) {
                holes.push(flat.len() / 2);
                flat.extend(hole.iter().flatten());
            }
        }
        let tris = earcutr::earcut(&flat, &holes, 2).map_err(|e| e.to_string())?;
        let start = cut.cap.vertices.len() as u32;
        cut.cap.vertices.extend(
            flat.as_chunks::<2>()
                .0
                .iter()
                .map(|p| (u * p[0] + v * p[1] + n * offset).to_array()),
        );
        for t in tris.as_chunks::<3>().0 {
            let a = start + t[0] as u32;
            let mut b = start + t[1] as u32;
            let mut c = start + t[2] as u32;
            let [pa, pb, pc] = [a, b, c].map(|k| DVec3::from(cut.cap.vertices[k as usize]));
            if (pb - pa).cross(pc - pa).dot(n) < 0. {
                std::mem::swap(&mut b, &mut c);
            }
            cut.cap.triangles.push([a, b, c]);
        }
    }
    Ok(cut)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Camera {
    pub fn set_view(&mut self, view: View) {
        (self.yaw, self.pitch) = match view {
            View::Iso => (-45., (1.0_f32 / 3.0_f32.sqrt()).asin().to_degrees()),
            View::Front => (-90., 0.),
            View::Back => (90., 0.),
            View::Left => (180., 0.),
            View::Right => (0., 0.),
            View::Top => (-90., 90.),
            View::Bottom => (-90., -90.),
            View::Edge(axes) | View::Corner(axes) => {
                let [x, y, z] = axes.map(f32::from);
                (y.atan2(x).to_degrees(), z.atan2(x.hypot(y)).to_degrees())
            }
        };
    }
    /// Zoom about the point at the target depth under the pointer.
    pub fn zoom_at(&mut self, aspect: f32, ndc: [f32; 2], factor: f32) {
        let (origin, ray) = self.ray(aspect, ndc);
        let normal = (self.target - self.eye()).normalize().as_dvec3();
        let depth = (self.target.as_dvec3() - origin).dot(normal) / ray.dot(normal);
        self.zoom_about((origin + ray * depth).as_vec3(), factor);
    }

    /// Keep a picked point at its screen position as the distance changes.
    pub fn zoom_about(&mut self, point: Vec3, factor: f32) {
        let distance = (self.distance * factor).clamp(1., 1e7);
        self.target += (point - self.target) * (1. - distance / self.distance);
        self.distance = distance;
    }
    pub fn fit(bodies: &[faber_ports::Body]) -> Self {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for b in bodies {
            lo = lo.min(Vec3::from(b.bounds[0].map(|v| v as f32)));
            hi = hi.max(Vec3::from(b.bounds[1].map(|v| v as f32)));
        }
        if bodies.is_empty() {
            lo = Vec3::ZERO;
            hi = Vec3::splat(100.);
        }
        Self {
            target: (lo + hi) / 2.,
            yaw: -55.,
            pitch: 24.,
            distance: (hi - lo).length().max(1.) * 1.8,
        }
    }
    pub fn eye(&self) -> Vec3 {
        let (s, c) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        self.target + Vec3::new(c * cp, s * cp, sp) * self.distance
    }
    fn reference_up(&self) -> Vec3 {
        if self.pitch.abs() >= 89.99 {
            Vec3::Y
        } else {
            Vec3::Z
        }
    }

    pub fn screen_axes(&self) -> (Vec3, Vec3) {
        let outward = (self.eye() - self.target).normalize();
        let right = self.reference_up().cross(outward).normalize();
        (right, outward.cross(right))
    }

    pub fn matrix(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(
            38f32.to_radians(),
            aspect.max(0.01),
            self.distance * 0.001,
            self.distance * 10. + 1000.,
        ) * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, self.reference_up())
    }
    pub fn ray(&self, aspect: f32, point: [f32; 2]) -> (DVec3, DVec3) {
        let inverse = self.matrix(aspect).inverse();
        let a = inverse
            .project_point3(Vec3::new(point[0], point[1], 0.))
            .as_dvec3();
        let b = inverse
            .project_point3(Vec3::new(point[0], point[1], 1.))
            .as_dvec3();
        (a, (b - a).normalize())
    }
}

pub fn ray_hits_bounds(bounds: [[f64; 3]; 2], origin: DVec3, direction: DVec3) -> bool {
    let (mut near, mut far) = (0.0_f64, f64::INFINITY);
    for axis in 0..3 {
        let lo = bounds[0][axis] - 1e-8;
        let hi = bounds[1][axis] + 1e-8;
        if direction[axis] == 0. {
            if origin[axis] < lo || origin[axis] > hi {
                return false;
            }
        } else {
            let a = (lo - origin[axis]) / direction[axis];
            let b = (hi - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if far < near {
                return false;
            }
        }
    }
    true
}

pub fn pick(mesh: &Mesh, origin: DVec3, direction: DVec3) -> Option<f64> {
    pick_visible(mesh, origin, direction, None)
}

pub fn pick_visible(
    mesh: &Mesh,
    origin: DVec3,
    direction: DVec3,
    plane: Option<Section>,
) -> Option<f64> {
    let clip = plane.map(|p| {
        let sign = if p.reverse { -1. } else { 1. };
        let normal = DVec3::from(p.normal).normalize() * sign;
        (normal.dot(origin) - sign * p.offset, normal.dot(direction))
    });
    let mut nearest = f64::INFINITY;
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let (e1, e2) = (b - a, c - a);
        let h = direction.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-10 {
            continue;
        }
        let s = origin - a;
        let u = s.dot(h) / det;
        let q = s.cross(e1);
        let v = direction.dot(q) / det;
        let distance = e2.dot(q) / det;
        if u >= 0.
            && v >= 0.
            && u + v <= 1.
            && distance > 0.
            && clip.is_none_or(|(base, slope)| base + slope * distance <= 1e-8)
        {
            nearest = nearest.min(distance);
        }
    }
    nearest.is_finite().then_some(nearest)
}

#[cfg(test)]
mod viewport_bounds_tests {
    use super::*;
    #[test]
    fn ray_skips_boxes_behind_or_beside_it() {
        let bounds = [[1., -1., -1.], [2., 1., 1.]];
        assert!(ray_hits_bounds(bounds, DVec3::ZERO, DVec3::X));
        assert!(!ray_hits_bounds(bounds, DVec3::ZERO, -DVec3::X));
        assert!(!ray_hits_bounds(bounds, DVec3::Y * 2., DVec3::X));
    }
}
