use crate::{
    Sky, TraceScene,
    material::{Brdf, Stream, cap},
};
use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use faber_scene::Surface;
use glam::{DVec3, Vec3};
use std::f64::consts::TAU;

struct Face {
    first: DVec3,
    ab: DVec3,
    ac: DVec3,
    normal: DVec3,
    normals: Option<[DVec3; 3]>,
    material: usize,
}
impl Face {
    fn normal(&self, p: DVec3) -> DVec3 {
        let Some([n0, n1, n2]) = self.normals else {
            return self.normal;
        };
        let ab = self.ab;
        let ac = self.ac;
        let q = p - self.first;
        let a = ab.dot(ab);
        let b = ab.dot(ac);
        let c = ac.dot(ac);
        let d = q.dot(ab);
        let e = q.dot(ac);
        let det = a * c - b * b;
        if det <= 0. {
            return self.normal;
        }
        let y = (c * d - b * e) / det;
        let z = (a * e - b * d) / det;
        let n = (n0 * (1. - y - z) + n1 * y + n2 * z)
            .try_normalize()
            .unwrap_or(self.normal);
        if n.dot(self.normal) < 0. { -n } else { n }
    }
}
struct Hit {
    p: DVec3,
    n: DVec3,
    geometric: DVec3,
    material: usize,
}
pub(crate) struct World {
    bvh: Option<Bvh>,
    faces: Vec<Face>,
    surfaces: Vec<Brdf>,
    sky: Sky,
    sun: DVec3,
    cos: f64,
    solid_angle: f32,
    radiance: Vec3,
    ground: Option<f64>,
    eps: f64,
}
impl World {
    pub(crate) fn new(s: &TraceScene) -> Result<Self> {
        let sky = s.sky;
        if s.ground.is_some_and(|z| !z.is_finite())
            || !(0. ..90.).contains(&sky.sun_radius_deg)
            || sky
                .horizon
                .iter()
                .chain(&sky.zenith)
                .chain(&sky.sun_radiance)
                .any(|c| !c.is_finite() || *c < 0.)
        {
            return Err(
                "Use finite sky radiance, ground height and a sun radius below 90 degrees.".into(),
            );
        }
        let sun = DVec3::from(sky.sun_direction)
            .try_normalize()
            .ok_or("Set a finite sun direction with non-zero length.")?;
        let mut mesh = Mesh::default();
        let mut faces = vec![];
        let mut surfaces = vec![];
        let mut low = DVec3::splat(f64::INFINITY);
        let mut high = -low;
        let n: usize = s
            .bodies
            .iter()
            .map(|b| b.mesh.triangles.len())
            .try_fold(0usize, usize::checked_add)
            .ok_or("Too many scene triangles.")?;
        if n > 5_000_000 {
            return Err(format!(
                "The scene has {n} triangles. The limit is 5000000."
            ));
        }
        for body in &s.bodies {
            let src = &body.mesh;
            let mat = body.surface;
            let index = surfaces.len();
            if mat
                .base
                .iter()
                .chain([&mat.roughness, &mat.metallic])
                .any(|c| !(0. ..=1.).contains(c))
            {
                return Err(
                    "Set surface reflectance, roughness and metallic values between 0 and 1."
                        .into(),
                );
            }
            if !src.normals.is_empty()
                && (src.normals.len() != src.vertices.len()
                    || src.normals.iter().flatten().any(|v| !v.is_finite()))
            {
                return Err("Set one finite surface normal per vertex.".into());
            }
            surfaces.push(Brdf::new(mat));
            let offset =
                u32::try_from(mesh.vertices.len()).map_err(|_| "Too many scene vertices.")?;
            if src.vertices.len() > u32::MAX as usize - mesh.vertices.len() {
                return Err("Too many scene vertices.".into());
            }
            mesh.vertices.extend_from_slice(&src.vertices);
            for tri in &src.triangles {
                let [Some(a), Some(b), Some(c)] =
                    tri.map(|i| src.vertices.get(i as usize).copied().map(DVec3::from))
                else {
                    return Err("Invalid triangle vertex index.".into());
                };
                if !a.is_finite() || !b.is_finite() || !c.is_finite() {
                    return Err("Mesh coordinates must be finite.".into());
                }
                let (ab, ac) = (b - a, c - a);
                let Some(normal) = ab.cross(ac).try_normalize() else {
                    continue;
                };
                let normals = (!src.normals.is_empty())
                    .then(|| tri.map(|i| DVec3::from(src.normals[i as usize])));
                low = low.min(a).min(b).min(c);
                high = high.max(a).max(b).max(c);
                mesh.triangles.push(tri.map(|i| i + offset));
                faces.push(Face {
                    first: a,
                    ab,
                    ac,
                    normal,
                    normals,
                    material: index,
                });
            }
        }
        let bvh = (!faces.is_empty()).then(|| Bvh::new(&mesh)).transpose()?;
        surfaces.push(Brdf::new(Surface {
            base: [0.5; 3],
            roughness: 0.9,
            metallic: 0.,
        }));
        let cos = sky.sun_radius_deg.to_radians().cos();
        Ok(Self {
            bvh,
            faces,
            surfaces,
            sky,
            sun,
            cos,
            solid_angle: (TAU * (1. - cos)) as f32,
            radiance: Vec3::from(sky.sun_radiance),
            ground: s.ground,
            eps: (high - low).max_element().max(1.) * 1e-9,
        })
    }
    fn ground_distance(&self, o: DVec3, d: DVec3) -> Option<f64> {
        let z = self.ground?;
        if d.z >= -1e-12 {
            return None;
        }
        let t = (z - o.z) / d.z;
        (t > 0.).then_some(t)
    }
    fn hit(&self, o: DVec3, d: DVec3) -> Option<Hit> {
        let mesh = self
            .bvh
            .as_ref()
            .and_then(|b| b.first_hit(o.to_array(), d.to_array()));
        let ground = self.ground_distance(o, d);
        if let Some(t) = ground
            && mesh.as_ref().is_none_or(|h| t < h.t)
        {
            return Some(Hit {
                p: o + d * t,
                n: DVec3::Z,
                geometric: DVec3::Z,
                material: self.surfaces.len() - 1,
            });
        }
        let h = mesh?;
        let p = DVec3::from(h.point);
        let face = &self.faces[h.triangle as usize];
        let side = if face.normal.dot(d) > 0. { -1. } else { 1. };
        let geometric = face.normal * side;
        let n = face.normal(p) * side;
        Some(Hit {
            p,
            geometric,
            n: if n.dot(d) < 0. { n } else { geometric },
            material: face.material,
        })
    }
    fn occluded(&self, o: DVec3, d: DVec3) -> bool {
        self.ground_distance(o, d).is_some()
            || self
                .bvh
                .as_ref()
                .is_some_and(|b| b.first_hit(o.to_array(), d.to_array()).is_some())
    }
    pub(crate) fn sample(&self, mut o: DVec3, mut d: DVec3, bounces: u32, r: &mut Stream) -> Vec3 {
        let mut weight = Vec3::ONE;
        let mut radiance = Vec3::ZERO;
        let mut see_sun = true;
        for depth in 0..=bounces {
            let Some(h) = self.hit(o, d) else {
                let env = Vec3::from(self.sky.horizon)
                    .lerp(Vec3::from(self.sky.zenith), d.z.clamp(0., 1.) as f32);
                let sun = if see_sun && d.dot(self.sun) > self.cos {
                    self.radiance
                } else {
                    Vec3::ZERO
                };
                radiance += weight * (env + sun);
                break;
            };
            if depth == bounces {
                break;
            }
            let mat = &self.surfaces[h.material];
            let eps = self.eps + 32. * f64::EPSILON * h.p.abs().max_element();
            o = h.p + h.geometric * eps;
            if !mat.mirror && self.radiance.max_element() > 0. && self.solid_angle > 0. {
                let l = cap(self.sun, self.cos, r);
                if h.n.dot(l) > 0. && h.geometric.dot(l) > 0. && !self.occluded(o, l) {
                    radiance +=
                        weight * mat.illuminated(h.n, -d, l) * self.radiance * self.solid_angle;
                }
            }
            let Some((l, w)) = mat.bounce(h.n, -d, r) else {
                break;
            };
            if l.dot(h.geometric) <= 0. {
                break;
            }
            weight *= w;
            if depth >= 2 {
                let keep = weight.max_element().clamp(0.05, 0.95);
                if r.next() > f64::from(keep) {
                    break;
                }
                weight /= keep;
            }
            d = l;
            see_sun = mat.mirror;
        }
        radiance
    }
}
