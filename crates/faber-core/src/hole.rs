use faber_model::{HoleHead, Placement};
use faber_ports::{Combine, Kernel, Result, Shape};

fn dimension(value: f64) -> Result<()> {
    if !value.is_finite() || !(1e-6..=1e6).contains(&value) {
        return Err("Hole dimensions must be between 0.000001 and 1000000 mm".into());
    }
    Ok(())
}

pub(crate) fn build(
    kernel: &dyn Kernel,
    source: &Shape,
    diameter: f64,
    depth: f64,
    placement: &Placement,
    head: Option<&HoleHead>,
) -> Result<Shape> {
    dimension(diameter)?;
    dimension(depth)?;
    let properties = kernel.summary_properties(source)?;
    if !properties.valid
        || properties.solids == 0
        || !properties.volume.is_finite()
        || properties.volume <= 0.
    {
        return Err("Hole source must be a valid solid".into());
    }
    let mut cutter = kernel.cylinder_shape(diameter / 2., depth)?;
    if let Some(head) = head {
        let (head_diameter, head_depth, angle) = match head {
            HoleHead::Counterbore { diameter, depth } => (*diameter, *depth, None),
            HoleHead::Countersink {
                diameter: head_diameter,
                angle,
            } => {
                if !angle.is_finite() || *angle <= 0. || *angle >= 180. {
                    return Err("Countersink angle must be between 0 and 180 degrees".into());
                }
                (
                    *head_diameter,
                    (head_diameter - diameter) / (2. * (angle.to_radians() / 2.).tan()),
                    Some(*angle),
                )
            }
        };
        dimension(head_diameter)?;
        dimension(head_depth)?;
        if head_diameter <= diameter || head_depth > depth {
            return Err("Hole head must be wider than the bore and no deeper than the hole".into());
        }
        let head = if angle.is_some() {
            kernel.cone_shape(head_diameter / 2., diameter / 2., head_depth)?
        } else {
            kernel.cylinder_shape(head_diameter / 2., head_depth)?
        };
        cutter = kernel.combine(&cutter, &head, Combine::Union)?;
    }
    let cutter = kernel.transform_shape(&cutter, placement)?;
    let result = kernel.combine(source, &cutter, Combine::Difference)?;
    if kernel.summary_properties(&result)?.volume >= properties.volume - 1e-8 {
        return Err("Hole does not remove material from the source".into());
    }
    Ok(result)
}
