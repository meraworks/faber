use faber_model::Placement;
use faber_ports::{Combine, Kernel, Result, Shape};

const FULL_TURN_TOLERANCE: f64 = 1e-9;

pub(crate) fn circular(
    kernel: &dyn Kernel,
    source: &Shape,
    count: f64,
    origin: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Result<Shape> {
    if !count.is_finite() || !(1. ..=1000.).contains(&count) || count.fract() != 0. {
        return Err("Pattern count must be an integer from 1 to 1000".into());
    }
    if origin
        .iter()
        .chain(&axis)
        .any(|value| !value.is_finite() || value.abs() > 1e6)
    {
        return Err("Pattern origin or axis is outside the supported range".into());
    }
    let axis = glam::DVec3::from_array(axis)
        .try_normalize()
        .ok_or("Pattern axis must be non-zero")?;
    if !angle.is_finite() || angle.abs() > 360. + FULL_TURN_TOLERANCE || (count > 1. && angle == 0.)
    {
        return Err("Pattern angle must be non-zero and between -360 and 360 degrees".into());
    }
    // A full turn divides the circle equally. Otherwise copy n is at n * angle.
    let full_turn = (angle.abs() - 360.).abs() <= FULL_TURN_TOLERANCE;
    let step = if full_turn { angle / count } else { angle };
    let properties = kernel.summary_properties(source)?;
    if !properties.valid
        || properties.solids == 0
        || !properties.volume.is_finite()
        || properties.volume <= 0.
    {
        return Err("Pattern source must be a valid solid".into());
    }
    let origin = glam::DVec3::from_array(origin);
    let mut result = source.clone();
    for index in 1..count as usize {
        let rotation = glam::DQuat::from_axis_angle(axis, (step * index as f64).to_radians());
        let (z, y, x) = rotation.to_euler(glam::EulerRot::ZYX);
        let copy = kernel.transform_shape(
            source,
            &Placement {
                translation: (origin - rotation * origin).to_array(),
                rotation: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
            },
        )?;
        result = kernel.combine(&result, &copy, Combine::Union)?;
    }
    Ok(result)
}

pub(crate) fn linear(
    kernel: &dyn Kernel,
    source: &Shape,
    count: f64,
    step: [f64; 3],
) -> Result<Shape> {
    if !count.is_finite() || !(1. ..=1000.).contains(&count) || count.fract() != 0. {
        return Err("Pattern count must be an integer from 1 to 1000".into());
    }
    if step
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1e6 || (value * (count - 1.)).abs() > 1e6)
        || (count > 1. && step.iter().all(|value| *value == 0.))
    {
        return Err("Pattern step must be non-zero and keep placements within 1000000 mm".into());
    }
    let properties = kernel.summary_properties(source)?;
    if !properties.valid
        || properties.solids == 0
        || !properties.volume.is_finite()
        || properties.volume <= 0.
    {
        return Err("Pattern source must be a valid solid".into());
    }
    let mut result = source.clone();
    for index in 1..count as usize {
        let copy = kernel.transform_shape(
            source,
            &Placement {
                translation: step.map(|value| value * index as f64),
                rotation: [0.; 3],
            },
        )?;
        result = kernel.combine(&result, &copy, Combine::Union)?;
    }
    Ok(result)
}
