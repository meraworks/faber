use faber_model::{FaceSelector, Placement, SketchLoop, SketchProfile};
use faber_ports::{Combine, Kernel, Result, Shape};
use glam::DVec3;

pub(crate) fn build(
    kernel: &dyn Kernel,
    source: &Shape,
    selector: &FaceSelector,
    size: &str,
    length: f64,
    offset: f64,
    left: bool,
) -> Result<Shape> {
    let metric = faber_std::metric(size).ok_or("Unknown metric thread size")?;
    let indices = crate::select::faces(kernel, source, selector)?;
    let [index] = indices.as_slice() else {
        return Err("Thread needs one cylindrical face".into());
    };
    let cylinder = kernel
        .faces(source)?
        .into_iter()
        .find(|f| f.index == *index)
        .and_then(|f| f.cylinder)
        .ok_or("Thread needs a cylindrical face")?;
    let pitch = metric.pitch();
    let major = metric.nominal() / 2.;
    let depth = 5. * 3f64.sqrt() * pitch / 16.;
    let radius = cylinder.radius;
    let minor = major - depth;
    if !length.is_finite()
        || !offset.is_finite()
        || length < pitch
        || offset < 0.
        || offset + length > cylinder.length + 1e-6
    {
        return Err("Thread needs at least one pitch within the selected face".into());
    }
    if (cylinder.internal && !(minor - 0.05 * pitch..major - 0.1 * pitch).contains(&radius))
        || (!cylinder.internal && (radius - major).abs() > 0.01 * pitch)
    {
        return Err("Cylinder diameter does not match the selected metric size".into());
    }
    let sign = if cylinder.internal { 1. } else { -1. };
    let cut_depth = if cylinder.internal {
        major - radius
    } else {
        depth
    };
    let overlap = pitch * 0.02;
    let root_half = pitch * if cylinder.internal { 0.0625 } else { 0.125 };
    let outer_half = root_half + (cut_depth + overlap) / 3f64.sqrt();
    let section = SketchProfile {
        outer: SketchLoop::Polygon {
            points: vec![
                [-sign * overlap, -outer_half],
                [sign * cut_depth, -root_half],
                [sign * cut_depth, root_half],
                [-sign * overlap, outer_half],
            ],
            corner_radius: None,
        },
        holes: vec![],
    };
    let p = if left { -pitch } else { pitch };
    let angle = (p / (std::f64::consts::TAU * radius)).atan().to_degrees() - 90.;
    let profile = kernel.sketch(
        &section,
        &Placement {
            translation: [radius, 0., 0.],
            rotation: [angle, 0., 0.],
        },
    )?;
    let path = kernel.helix(radius, p, length / pitch + 2.)?;
    let cutter = kernel.sweep_shape(&profile, &path, true)?;
    let cutter = kernel.transform_shape(
        &cutter,
        &Placement {
            translation: [0., 0., if left { length + pitch } else { -pitch }],
            ..Default::default()
        },
    )?;
    let limit = kernel.cylinder_shape(major + pitch, length)?;
    let cutter = kernel.combine(&cutter, &limit, Combine::Intersection)?;
    let axis = DVec3::from(cylinder.axis);
    let placement = Placement {
        translation: (DVec3::from(cylinder.start) + axis * offset).to_array(),
        rotation: [
            0.,
            axis.z.clamp(-1., 1.).acos().to_degrees(),
            axis.y.atan2(axis.x).to_degrees(),
        ],
    };
    kernel.combine(
        source,
        &kernel.transform_shape(&cutter, &placement)?,
        Combine::Difference,
    )
}
