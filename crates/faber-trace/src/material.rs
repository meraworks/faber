use crate::frame;
use faber_scene::Surface;
use glam::{DVec3, Vec3};
use std::f64::consts::{PI, TAU};
pub(crate) struct Stream(u64);
fn mix(mut v: u64) -> u64 {
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}
impl Stream {
    pub(crate) fn new(seed: u64, pixel: u64, sample: u32) -> Self {
        Self(mix(seed
            ^ mix(pixel.wrapping_add(1))
            ^ u64::from(sample).wrapping_mul(0x9e3779b97f4a7c15)))
    }
    pub(crate) fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        ((mix(self.0) >> 11) as f64 + 0.5) * (1. / 9_007_199_254_740_992.)
    }
}
fn around(axis: DVec3, cos: f64, phi: f64) -> DVec3 {
    let (x, y) = frame(axis);
    let (s, c) = phi.sin_cos();
    let sin = (1. - cos * cos).max(0.).sqrt();
    x * (sin * c) + y * (sin * s) + axis * cos
}
pub(crate) fn cap(axis: DVec3, cos_min: f64, r: &mut Stream) -> DVec3 {
    around(axis, 1. - r.next() * (1. - cos_min), TAU * r.next())
}
pub(crate) struct Brdf {
    base: Vec3,
    f0: Vec3,
    metal: f32,
    a2: f64,
    prob: f64,
    pub(crate) mirror: bool,
}
impl Brdf {
    pub(crate) fn new(s: Surface) -> Self {
        let base = Vec3::from(s.base);
        let metal = s.metallic;
        Self {
            base,
            f0: Vec3::splat(0.04).lerp(base, metal),
            metal,
            a2: f64::from(s.roughness.max(0.025)).powi(4),
            prob: 0.2 + 0.8 * f64::from(metal),
            mirror: metal > 0.999 && s.roughness < 0.025,
        }
    }
    fn eval_pdf(&self, n: DVec3, v: DVec3, l: DVec3) -> (Vec3, f64) {
        let nv = n.dot(v).max(1e-7);
        let nl = n.dot(l);
        if nl <= 0. {
            return (Vec3::ZERO, 0.);
        }
        let Some(h) = (l + v).try_normalize() else {
            return (Vec3::ZERO, 0.);
        };
        let nh = n.dot(h).max(0.);
        let vh = v.dot(h).max(1e-7);
        let a = self.a2;
        let d = a / (PI * (nh * nh * (a - 1.) + 1.).powi(2));
        let vis = 0.5
            / (nl * (a + (1. - a) * nv * nv).sqrt() + nv * (a + (1. - a) * nl * nl).sqrt())
                .max(1e-14);
        let fres = self.f0 + (Vec3::ONE - self.f0) * (1. - vh).powi(5) as f32;
        let diffuse = self.base * ((1. - self.metal) / std::f32::consts::PI) * (Vec3::ONE - fres);
        (
            diffuse + fres * (d * vis) as f32,
            (1. - self.prob) * nl / PI + self.prob * d * nh / (4. * vh),
        )
    }
    pub(crate) fn illuminated(&self, n: DVec3, v: DVec3, l: DVec3) -> Vec3 {
        self.eval_pdf(n, v, l).0 * n.dot(l).max(0.) as f32
    }
    pub(crate) fn bounce(&self, n: DVec3, v: DVec3, r: &mut Stream) -> Option<(DVec3, Vec3)> {
        if self.mirror {
            return Some((2. * v.dot(n) * n - v, self.base));
        }
        let l = if r.next() < self.prob {
            let u = r.next();
            let cos = ((1. - u) / (1. + (self.a2 - 1.) * u)).sqrt();
            let h = around(n, cos, TAU * r.next());
            let vh = v.dot(h);
            if vh <= 0. {
                return None;
            }
            2. * vh * h - v
        } else {
            around(n, r.next().sqrt(), TAU * r.next())
        };
        let (f, p) = self.eval_pdf(n, v, l);
        if p <= 0. {
            return None;
        }
        Some((l, f * (n.dot(l) / p) as f32))
    }
}
