//! Round bars and tubes along a path.

use faber_model::{Placement, SketchLoop, SketchProfile};
use faber_ports::{Kernel, Result, Shape};
use glam::{DQuat, DVec3, EulerRot};

/// Sweeps a circle along `path`. The circle is normal to the path start.
pub(crate) fn build(
    kernel: &dyn Kernel,
    path: &Shape,
    diameter: f64,
    wall: Option<f64>,
) -> Result<Shape> {
    if !diameter.is_finite() || !(1e-6..=1e6).contains(&diameter) {
        return Err("Pipe diameter must be between 0.000001 and 1000000 mm".into());
    }
    let radius = diameter / 2.;
    let bore = match wall {
        None => None,
        Some(wall) if !wall.is_finite() || wall <= 0. => {
            return Err(format!("Pipe wall {wall} mm must be more than zero."));
        }
        Some(wall) if wall >= radius => {
            return Err(format!(
                "Pipe wall {wall} mm must be less than the radius {radius} mm."
            ));
        }
        Some(wall) => Some(radius - wall),
    };
    let start = kernel.path_samples(path, &[0.])?;
    let sample = start.samples.first().ok_or("Path has no start")?;
    let tangent = DVec3::from_array(sample.tangent)
        .try_normalize()
        .ok_or("Path has no tangent at its start")?;
    let rotation = DQuat::from_rotation_arc(DVec3::Z, tangent);
    let (z, y, x) = rotation.to_euler(EulerRot::ZYX);
    let placement = Placement {
        translation: sample.point,
        rotation: [x, y, z].map(f64::to_degrees),
    };
    let circle = |radius| SketchLoop::Circle {
        centre: [0., 0.],
        radius,
    };
    let profile = SketchProfile {
        outer: circle(radius),
        holes: bore.map(circle).into_iter().collect(),
    };
    let section = kernel.sketch(&profile, &placement)?;
    kernel.sweep_shape(&section, path, false)
}
