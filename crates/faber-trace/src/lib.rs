//! Deterministic CPU path tracing. Colour channels use linear radiance.
mod camera;
mod integrate;
mod material;

use faber_model::view::Camera;
use faber_ports::{Mesh, Result};
use faber_scene::Surface;
use glam::{DVec3, Vec3};
use rayon::prelude::*;
use std::{sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct TraceBody {
    pub mesh: Arc<Mesh>,
    pub surface: Surface,
}
#[derive(Clone, Copy, Debug)]
pub struct Sky {
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
    pub sun_direction: [f64; 3],
    pub sun_radius_deg: f64,
    pub sun_radiance: [f32; 3],
}
impl Default for Sky {
    fn default() -> Self {
        Self {
            horizon: [0.42, 0.48, 0.56],
            zenith: [0.09, 0.16, 0.29],
            sun_direction: [-0.5, -0.65, 1.],
            sun_radius_deg: 5.,
            sun_radiance: [180., 155., 129.],
        }
    }
}
#[derive(Clone, Debug)]
pub struct TraceScene {
    pub bodies: Vec<TraceBody>,
    pub camera: Camera,
    pub sky: Sky,
    pub ground: Option<f64>,
}
#[derive(Clone, Copy, Debug)]
pub struct TraceOptions {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub max_bounces: u32,
    pub seconds: Option<f64>,
    pub seed: u64,
}
impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            samples: 64,
            max_bounces: 6,
            seconds: None,
            seed: 1,
        }
    }
}
#[derive(Debug)]
pub struct Traced {
    pub pixels: Vec<[f32; 3]>,
    pub samples_done: u32,
    pub seconds: f64,
}
pub fn trace(scene: &TraceScene, o: &TraceOptions) -> Result<Traced> {
    let started = Instant::now();
    let count = u64::from(o.width) * u64::from(o.height);
    if count == 0 || count > 16_000_000 {
        return Err(format!("The image has {count} pixels. Use 1 to 16000000."));
    }
    if !(1..=1_000_000).contains(&o.samples) || !(1..=64).contains(&o.max_bounces) {
        return Err("Use 1 to 1000000 samples and 1 to 64 bounces.".into());
    }
    if o.seconds.is_some_and(|s| !s.is_finite() || s <= 0.) {
        return Err("Set a positive finite time limit.".into());
    }
    let camera = camera::Frame::new(&scene.camera, o.width, o.height)?;
    let world = integrate::World::new(scene)?;
    let mut pixels = vec![[0.; 3]; count as usize];
    let mut done = 0;
    for s in 0..o.samples {
        pixels
            .par_iter_mut()
            .enumerate()
            .try_for_each(|(index, acc)| {
                let mut rng = material::Stream::new(o.seed, index as u64, s);
                let (p, d) = camera.ray(index as u32, &mut rng);
                let v = world.sample(p, d, o.max_bounces, &mut rng);
                if !v.is_finite() {
                    return Err("The light radiance exceeds the representable range.");
                }
                let mean = Vec3::from(*acc);
                *acc = (mean + (v - mean) * (1. / (s + 1) as f32)).to_array();
                Ok(())
            })?;
        done += 1;
        if o.seconds
            .is_some_and(|s| started.elapsed().as_secs_f64() >= s)
        {
            break;
        }
    }
    Ok(Traced {
        pixels,
        samples_done: done,
        seconds: started.elapsed().as_secs_f64(),
    })
}
/// Apply exposure and a Reinhard curve, then encode sRGB bytes.
pub fn tone_map(pixels: &[[f32; 3]], exposure: f32) -> Vec<[u8; 3]> {
    pixels
        .iter()
        .map(|p| {
            p.map(|c| {
                let x = f64::from(c.max(0.)) * f64::from(exposure.max(0.));
                let m = x / (1. + x);
                let s = if m <= 0.0031308 {
                    12.92 * m
                } else {
                    1.055 * m.powf(1. / 2.4) - 0.055
                };
                (255. * s).round().clamp(0., 255.) as u8
            })
        })
        .collect()
}
fn frame(n: DVec3) -> (DVec3, DVec3) {
    let x = n.any_orthonormal_vector();
    (x, n.cross(x))
}
