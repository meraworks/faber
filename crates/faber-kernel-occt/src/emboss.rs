use super::{pointer, shape_call};
use faber_ports::{CylinderFace, Result, Shape};
use std::ffi::{c_char, c_void};

unsafe extern "C" {
    fn faber_wrap_cylinder(
        flat: *const c_void,
        start: *const f64,
        axis: *const f64,
        radius: f64,
        length: f64,
        internal: i32,
        at: *const f64,
        output: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
}
pub(super) fn wrap(flat: &Shape, cylinder: &CylinderFace, at: [f64; 2]) -> Result<Shape> {
    if !at.into_iter().all(f64::is_finite) || !cylinder.radius.is_finite() || cylinder.radius <= 0.
    {
        return Err("Cylinder wrapping needs a positive radius and finite offsets.".into());
    }
    let input = pointer(flat)?;
    shape_call(|out, error, size| unsafe {
        faber_wrap_cylinder(
            input,
            cylinder.start.as_ptr(),
            cylinder.axis.as_ptr(),
            cylinder.radius,
            cylinder.length,
            i32::from(cylinder.internal),
            at.as_ptr(),
            out,
            error,
            size,
        )
    })
}
