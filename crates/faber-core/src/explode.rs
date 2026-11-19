//! Part displacements for an exploded assembly view.
use faber_model::motion::MotionStudy;
use faber_ports::{Body, Result};
use glam::DVec3;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Offset {
    pub part: String,
    pub vector: [f64; 3],
}

fn centre(body: &Body) -> DVec3 {
    (DVec3::from(body.bounds[0]) + DVec3::from(body.bounds[1])) * 0.5
}

/// Find the interval along the ray in which the two boxes touch.
fn interval(
    bounds: [[f64; 3]; 2],
    used: [[f64; 3]; 2],
    dir: DVec3,
    gap: f64,
) -> Option<(f64, f64)> {
    let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..3 {
        let a = used[0][i] - gap - bounds[1][i];
        let b = used[1][i] + gap - bounds[0][i];
        if dir[i].abs() < 1e-12 {
            if a > 0. || b < 0. {
                return None;
            }
        } else {
            let (lo, hi) = (a / dir[i], b / dir[i]);
            near = near.max(lo.min(hi));
            far = far.min(lo.max(hi));
        }
    }
    (near <= far).then_some((near, far))
}

/// Keep the largest part at its assembled position. Return world offsets.
pub fn offsets(bodies: &[Body], motion: &MotionStudy, factor: f64) -> Result<Vec<Offset>> {
    if !factor.is_finite() || !(0.0..=1.).contains(&factor) {
        return Err("The explode factor must be between 0 and 1.".into());
    }
    if bodies.len() < 2 || factor == 0. {
        return Ok(Vec::new());
    }
    let fixed = bodies
        .iter()
        .max_by(|a, b| a.volume.total_cmp(&b.volume).then_with(|| b.id.cmp(&a.id)))
        .unwrap();
    let origin = centre(fixed);
    let mut parts: Vec<_> = bodies.iter().filter(|b| b.id != fixed.id).collect();
    parts.sort_by(|a, b| {
        centre(a)
            .distance_squared(origin)
            .total_cmp(&centre(b).distance_squared(origin))
            .then_with(|| a.id.cmp(&b.id))
    });
    let axes: BTreeMap<_, _> = motion
        .joints
        .values()
        .flat_map(|j| {
            j.parts
                .iter()
                .map(move |id| (id.as_str(), DVec3::from(j.axis)))
        })
        .collect();
    let mut used = vec![fixed.bounds];
    let mut result = Vec::with_capacity(parts.len());
    for body in parts {
        let radial = centre(body) - origin;
        let dir = if let Some(axis) = axes.get(body.id.as_str()) {
            let axis = axis
                .try_normalize()
                .ok_or("The joint axis has no direction.")?;
            if axis.dot(radial) < 0. { -axis } else { axis }
        } else {
            let unit = radial.try_normalize().unwrap_or(DVec3::Z);
            let index = unit.abs().max_position();
            if unit[index].abs() >= 30f64.to_radians().cos() {
                DVec3::AXES[index] * unit[index].signum()
            } else {
                unit
            }
        };
        let span = DVec3::from(body.bounds[1]) - DVec3::from(body.bounds[0]);
        let gap = (0.2 * dir.abs().dot(span)).max(1e-6);
        let mut distance = gap;
        let mut intervals: Vec<_> = used
            .iter()
            .filter_map(|b| interval(body.bounds, *b, dir, gap))
            .collect();
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (near, far) in intervals {
            if near > distance {
                break;
            }
            distance = distance.max(far);
        }
        let full = dir * distance;
        used.push(body.bounds.map(|p| (DVec3::from(p) + full).to_array()));
        result.push(Offset {
            part: body.id.clone(),
            vector: (factor * full).to_array(),
        });
    }
    Ok(result)
}
