//! Build patch faces and join face sets.
use super::{pointer, shape_call};
use faber_ports::{Result, Shape, Stitched};
use std::ffi::{c_char, c_int, c_void};
unsafe extern "C" {
    fn faber_join_edge(
        input: *const c_void,
        index: u32,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> c_int;
    fn faber_patch_boundary(
        edges: *const *const c_void,
        count: usize,
        support: *const *const c_void,
        supports: usize,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> c_int;
    fn faber_stitch_faces(
        faces: *const *const c_void,
        count: usize,
        tolerance: f64,
        solid: c_int,
        open: *mut u32,
        gap: *mut f64,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> c_int;
}
pub(super) fn edge(shape: &Shape, index: u32) -> Result<Shape> {
    let input = pointer(shape)?;
    shape_call(|out, error, len| unsafe { faber_join_edge(input, index, out, error, len) })
}
fn pointers(shapes: &[Shape]) -> Result<Vec<*const c_void>> {
    shapes.iter().map(pointer).collect()
}
pub(super) fn patch(boundary: &[Shape], support: &[Shape]) -> Result<Shape> {
    let edges = pointers(boundary)?;
    let faces = pointers(support)?;
    shape_call(|out, error, len| unsafe {
        faber_patch_boundary(
            edges.as_ptr(),
            edges.len(),
            faces.as_ptr(),
            faces.len(),
            out,
            error,
            len,
        )
    })
}
pub(super) fn stitch(surfaces: &[Shape], tolerance: f64, as_solid: bool) -> Result<Stitched> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err("The stitch tolerance must be finite and positive.".into());
    }
    let faces = pointers(surfaces)?;
    let mut open_edges = 0;
    let mut largest_gap = 0.;
    let shape = shape_call(|out, error, len| unsafe {
        faber_stitch_faces(
            faces.as_ptr(),
            faces.len(),
            tolerance,
            i32::from(as_solid),
            &mut open_edges,
            &mut largest_gap,
            out,
            error,
            len,
        )
    })?;
    Ok(Stitched {
        shape,
        open_edges,
        largest_gap,
        solid: as_solid,
    })
}
