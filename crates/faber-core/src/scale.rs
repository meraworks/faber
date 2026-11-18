//! Scale of a solid about a point.

use faber_model::ScaleFactor;
use faber_ports::{Kernel, Result, Shape};

pub(crate) fn build(
    kernel: &dyn Kernel,
    source: &Shape,
    factor: &ScaleFactor,
    about: [f64; 3],
) -> Result<Shape> {
    let factor = match factor {
        ScaleFactor::Uniform(value) => [*value; 3],
        ScaleFactor::Axes(values) => *values,
    };
    if factor.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("Scale factor must be finite and more than zero".into());
    }
    kernel.scale_shape(source, about, factor)
}
