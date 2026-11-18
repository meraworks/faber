//! Checks and face lookup for the chamfer and fillet variants.
//! The checks run before the kernel, so a bad value is refused early.

use faber_model::FaceSelector;
use faber_ports::{Kernel, Result, Shape};

/// Refuse a chamfer that cannot be built.
pub(crate) fn check_chamfer(
    distance: f64,
    distance2: Option<f64>,
    angle_deg: Option<f64>,
) -> Result<()> {
    positive("Chamfer distance", distance)?;
    match (distance2, angle_deg) {
        (Some(_), Some(_)) => Err("Set distance2 or angle_deg, not both.".into()),
        (Some(second), None) => positive("Chamfer distance2", second),
        (None, Some(angle)) => {
            positive("Chamfer angle_deg", angle)?;
            if angle < 90. {
                Ok(())
            } else {
                Err("Chamfer angle_deg must be less than 90.".into())
            }
        }
        (None, None) => Ok(()),
    }
}

/// Refuse a fillet that cannot be built.
pub(crate) fn check_fillet(radius: f64, radius_end: Option<f64>) -> Result<()> {
    positive("Fillet radius", radius)?;
    match radius_end {
        Some(end) => positive("Fillet radius_end", end),
        None => Ok(()),
    }
}

/// Return the face index that the chamfer face selector matches.
pub(crate) fn chamfer_face(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &FaceSelector,
) -> Result<u32> {
    let matched = crate::select::faces(kernel, shape, selector)?;
    match matched.as_slice() {
        [index] => Ok(*index),
        _ => Err("Chamfer face must match one face.".into()),
    }
}

fn positive(name: &str, value: f64) -> Result<()> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(format!("{name} must be positive."))
    }
}
