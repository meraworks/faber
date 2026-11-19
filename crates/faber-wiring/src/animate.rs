//! Render motion and camera frames from one project snapshot.
mod framing;
use crate::{
    studio::{self, Style},
    views::{self, Picture},
};
use faber_core::Project;
use faber_model::{motion::PoseValue, view::Camera};
use faber_ports::Result;
use faber_ports::animation::{AnimationWriter, Sequence};
use glam::{DMat3, DQuat, DVec3};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Animation {
    Turntable { turns: f64 },
    Explode { from: f64, to: f64 },
    Motion { joint: String, from: f64, to: f64 },
    Views { names: Vec<String> },
}
#[derive(Clone, Debug)]
pub struct AnimationOptions {
    pub frames: u32,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub style: Style,
    pub view: Option<String>,
    pub frames_dir: Option<PathBuf>,
}
impl Default for AnimationOptions {
    fn default() -> Self {
        Self {
            frames: 48,
            fps: 24,
            width: 640,
            height: 480,
            style: Style::Technical,
            view: None,
            frames_dir: None,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct AnimationReport {
    pub path: PathBuf,
    pub frames: u32,
    pub duration: f64,
}
fn orient(c: Camera) -> (DQuat, f64) {
    let outward = DVec3::from(c.eye) - DVec3::from(c.target);
    let z = outward.normalize();
    let x = DVec3::from(c.up).cross(z).normalize();
    let y = z.cross(x);
    (
        DQuat::from_mat3(&DMat3::from_cols(x, y, z)),
        outward.length(),
    )
}
fn interpolate(a: Camera, b: Camera, t: f64) -> Camera {
    if t == 0. {
        return a;
    }
    if t == 1. {
        return b;
    }
    let (qa, da) = orient(a);
    let (qb, db) = orient(b);
    let q = qa.slerp(qb, t);
    let target = DVec3::from(a.target).lerp(b.target.into(), t);
    let d = da + (db - da) * t;
    let height = |c: Camera, dist: f64| {
        c.ortho_height
            .unwrap_or_else(|| 2. * dist * (0.5 * c.fov_deg.unwrap().to_radians()).tan())
    };
    let ha = height(a, da);
    let hb = height(b, db);
    let h = ha + (hb - ha) * t;
    let mut out = Camera {
        target: target.to_array(),
        eye: (target + q * DVec3::Z * d).to_array(),
        up: (q * DVec3::Y).to_array(),
        ortho_height: Some(h),
        fov_deg: None,
    };
    if a.fov_deg.is_some() || b.fov_deg.is_some() {
        // Interpolate inverse distance. Zero is an orthographic projection.
        let inv = (if a.fov_deg.is_some() { 1. / da } else { 0. }) * (1. - t)
            + (if b.fov_deg.is_some() { 1. / db } else { 0. }) * t;
        let dist = 1. / inv;
        out.eye = (target + q * DVec3::Z * dist).to_array();
        out.fov_deg = Some(2. * (h / (2. * dist)).atan().to_degrees());
        out.ortho_height = None;
    }
    out
}
fn check(project: &Project, animation: &Animation, opts: &AnimationOptions) -> Result<()> {
    if opts.frames < 2 {
        return Err("An animation needs 2 frames or more.".into());
    }
    if opts.frames > 600 || !(1..=240).contains(&opts.fps) {
        return Err("Use at most 600 frames and 1 to 240 frames per second.".into());
    }
    if !(64..=4096).contains(&opts.width) || !(64..=4096).contains(&opts.height) {
        return Err("Image size must be between 64 and 4096 pixels".into());
    }
    match animation {
        Animation::Turntable { turns } if !turns.is_finite() => {
            return Err("The number of turns must be finite.".into());
        }
        Animation::Explode { from, to } if ![from, to].iter().all(|n| (0.0..=1.).contains(*n)) => {
            return Err("Set both explode factors between 0 and 1.".into());
        }
        Animation::Motion { joint, from, to } => {
            let j = project
                .data
                .motion
                .joints
                .get(joint)
                .ok_or_else(|| format!("Unknown joint: {joint}"))?;
            if j.kind.count() != 1 {
                return Err(format!("Joint {joint} must have one motion coordinate."));
            }
            for &n in [from, to] {
                if !n.is_finite() {
                    return Err("The joint coordinate must be finite.".into());
                }
                if let Some([lo, hi]) = j.limits
                    && !(lo..=hi).contains(&n)
                {
                    return Err(format!("Joint {joint} has limits {lo} to {hi}."));
                }
            }
        }
        Animation::Views { names } => {
            if names.len() < 2 {
                return Err("Choose at least two named views.".into());
            }
            for name in names {
                if !project.data.views.contains_key(name) {
                    return Err(format!("Unknown view: {name}"));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn animate(
    project: &Project,
    animation: &Animation,
    opts: &AnimationOptions,
    path: &Path,
) -> Result<AnimationReport> {
    let options = studio::Options {
        style: opts.style,
        ..Default::default()
    };
    with_studio(project, animation, opts, path, &options)
}
pub fn with_studio(
    project: &Project,
    animation: &Animation,
    opts: &AnimationOptions,
    path: &Path,
    options: &studio::Options,
) -> Result<AnimationReport> {
    check(project, animation, opts)?;
    let base = Picture {
        view: opts.view.clone(),
        style: Some(opts.style),
        ..Default::default()
    };
    let original = views::prepare(project, &base)?;
    let camera = framing::camera(project, animation, opts, &original)?;
    let seq = Sequence {
        width: opts.width,
        height: opts.height,
        frames: opts.frames,
        fps: opts.fps,
    };
    faber_files::apng::Writer.write(path, opts.frames_dir.as_deref(), &seq, &mut |i| {
        let t = f64::from(i) / f64::from(opts.frames - 1);
        let mut pic = base.clone();
        let mut cam = camera;
        let scene;
        let bodies = match animation {
            Animation::Turntable { turns } => {
                let phase = (t * turns).rem_euclid(1.);
                if phase != 0. {
                    let q = DQuat::from_rotation_z(phase * std::f64::consts::TAU);
                    let target = DVec3::from(camera.target);
                    cam.eye = (target + q * (DVec3::from(camera.eye) - target)).to_array();
                    cam.up = (q * DVec3::from(camera.up)).to_array();
                }
                &original.bodies
            }
            Animation::Views { names } => {
                let pos = t * (names.len() - 1) as f64;
                let a = pos.floor() as usize;
                let b = (a + 1).min(names.len() - 1);
                pic.view = Some(names[a].clone());
                let first = &project.data.views[&names[a]];
                let next = &project.data.views[&names[b]];
                let fraction = pos.fract();
                cam = interpolate(first.camera, next.camera, fraction);
                if fraction > 0. {
                    let start = views::pose(&project.data, &names[a])?;
                    let end = views::pose(&project.data, &names[b])?;
                    for (id, joint) in &project.data.motion.joints {
                        let (u, v) = (start.get(id), end.get(id));
                        if u.is_none() && v.is_none() {
                            continue;
                        }
                        let zeros = [0.; 3];
                        let rest = &zeros[..joint.kind.count()];
                        let (u, v) = (
                            u.map_or(rest, PoseValue::as_slice),
                            v.map_or(rest, PoseValue::as_slice),
                        );
                        if u.len() != rest.len() || v.len() != rest.len() {
                            return Err(format!(
                                "The saved pose has the wrong number of coordinates for joint {id}."
                            ));
                        }
                        let values = u
                            .iter()
                            .zip(v)
                            .map(|(u, v)| u + (v - u) * fraction)
                            .collect();
                        pic.joints
                            .insert(id.clone(), PoseValue::from_values(values));
                    }
                    let start = first.explode.unwrap_or(0.);
                    pic.explode = Some(start + (next.explode.unwrap_or(0.) - start) * fraction);
                }
                scene = views::prepare(project, &pic)?;
                &scene.bodies
            }
            Animation::Explode { from, to } => {
                pic.explode = Some(from + (to - from) * t);
                scene = views::prepare(project, &pic)?;
                &scene.bodies
            }
            Animation::Motion { joint, from, to } => {
                pic.joints
                    .insert(joint.clone(), PoseValue::Number(from + (to - from) * t));
                scene = views::prepare(project, &pic)?;
                &scene.bodies
            }
        };
        Ok(studio::draw(
            &project.data,
            bodies,
            &cam,
            options,
            opts.width,
            opts.height,
        )?
        .rgba)
    })?;
    Ok(AnimationReport {
        path: path.to_owned(),
        frames: opts.frames,
        duration: f64::from(opts.frames) / f64::from(opts.fps),
    })
}
