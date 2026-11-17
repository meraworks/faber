//! Camera and section geometry shared by both views.

use faber_ports::{Mesh, Result};
use glam::{DVec3, Mat4, Vec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub normal: [f64; 3],
    pub offset: f64,
    #[serde(default)]
    pub reverse: bool,
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
}

pub fn section(mesh: &Mesh, plane: Section) -> Result<Cut> {
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
    for t in &mesh.triangles {
        let points = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let distances = points.map(|p| n.dot(p) - offset);
        let mut polygon = Vec::new();
        let mut crossings = Vec::new();
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (a, b, da, db) = (points[i], points[j], distances[i], distances[j]);
            if da <= 1e-8 {
                polygon.push(a);
            }
            if (da < -1e-8 && db > 1e-8) || (da > 1e-8 && db < -1e-8) {
                let p = a + (b - a) * (da / (da - db));
                polygon.push(p);
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
    let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
    while let Some(&(a, b)) = edges.first() {
        edges.remove(&(a, b));
        let mut path = vec![a, b];
        let mut at = b;
        while at != a {
            let edge = edges
                .iter()
                .find(|(s, e)| *s == at || *e == at)
                .copied()
                .ok_or("Section boundary is open")?;
            edges.remove(&edge);
            at = if edge.0 == at { edge.1 } else { edge.0 };
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
            View::Iso => (-55., 24.),
            View::Front => (-90., 0.),
            View::Back => (90., 0.),
            View::Left => (180., 0.),
            View::Right => (0., 0.),
            View::Top => (-90., 90.),
            View::Bottom => (-90., -90.),
        };
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
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let up = if self.pitch.abs() >= 89.99 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        glam::camera::rh::proj::directx::perspective(
            38f32.to_radians(),
            aspect.max(0.01),
            self.distance * 0.001,
            self.distance * 10. + 1000.,
        ) * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, up)
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

pub fn pick(mesh: &Mesh, origin: DVec3, direction: DVec3) -> Option<f64> {
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
        if u >= 0. && v >= 0. && u + v <= 1. && distance > 0. {
            nearest = nearest.min(distance);
        }
    }
    nearest.is_finite().then_some(nearest)
}
