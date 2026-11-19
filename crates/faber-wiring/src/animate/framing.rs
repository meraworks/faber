//! Fit one stationary camera to all positions shown in the animation.
use super::{Animation, AnimationOptions};
use crate::views::Prepared;
use faber_core::Project;
use faber_model::{
    motion::{Pose, PoseValue},
    view::Camera,
};
use faber_ports::Result;
use glam::{DAffine3, DQuat, DVec3, EulerRot};
use std::collections::BTreeMap;

pub(super) fn camera(
    project: &Project,
    animation: &Animation,
    opts: &AnimationOptions,
    original: &Prepared,
) -> Result<Camera> {
    let mut camera = original.camera_at_size(opts.width, opts.height);
    if opts.view.is_some() || original.bodies.is_empty() {
        return Ok(camera);
    }
    let mut low = DVec3::splat(f64::INFINITY);
    let mut high = -low;
    for body in &original.bodies {
        low = low.min(body.bounds[0].into());
        high = high.max(body.bounds[1].into());
    }
    let assembled = (low, high);
    match animation {
        Animation::Explode { .. } => {
            // The same framing applies to any subrange of the explode control.
            let offsets: BTreeMap<_, _> =
                faber_core::explode::offsets(&project.bodies, &project.data.motion, 1.)?
                    .into_iter()
                    .map(|o| (o.part, DVec3::from(o.vector)))
                    .collect();
            for body in &original.bodies {
                if let Some(v) = offsets.get(&body.id) {
                    low = low.min(DVec3::from(body.bounds[0]) + *v);
                    high = high.max(DVec3::from(body.bounds[1]) + *v);
                }
            }
        }
        Animation::Motion { joint, from, to } => {
            for i in 0..opts.frames {
                let t = f64::from(i) / f64::from(opts.frames - 1);
                let pose = Pose::from([(joint.clone(), PoseValue::Number(from + (to - from) * t))]);
                let solved = project.data.motion.solve_pose(&pose)?;
                for body in &original.bodies {
                    let Some(p) = solved.get(&body.id) else {
                        continue;
                    };
                    let [x, y, z] = p.rotation.map(f64::to_radians);
                    let xf = DAffine3::from_rotation_translation(
                        DQuat::from_euler(EulerRot::ZYX, z, y, x),
                        p.translation.into(),
                    );
                    let [a, b] = body.bounds;
                    for x in [a[0], b[0]] {
                        for y in [a[1], b[1]] {
                            for z in [a[2], b[2]] {
                                let corner = xf.transform_point3(DVec3::new(x, y, z));
                                low = low.min(corner);
                                high = high.max(corner);
                            }
                        }
                    }
                }
            }
        }
        _ => return Ok(camera),
    }
    if assembled == (low, high) {
        return Ok(camera);
    }
    let direction = (DVec3::from(camera.eye) - DVec3::from(camera.target)).normalize();
    let target = (low + high) * 0.5;
    let half_fov = camera
        .fov_deg
        .ok_or("The automatic camera needs a field of view.")?
        .to_radians()
        * 0.5;
    let aspect = f64::from(opts.width) / f64::from(opts.height);
    let half_angle = half_fov.min((half_fov.tan() * aspect).atan());
    // A sphere around the entire envelope fits in both camera planes.
    let distance = 0.55 * (high - low).length().max(1.) / half_angle.sin();
    camera.target = target.to_array();
    camera.eye = (target + direction * distance).to_array();
    camera.validate()?;
    Ok(camera)
}
