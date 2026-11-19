use super::{Result, Shape, c_char, c_void, pointer, shape_call};
use faber_model::SplitKeep;

unsafe extern "C" {
    fn faber_trim_surface(
        input: *const c_void,
        tool: *const c_void,
        mode: i32,
        origin: *const f64,
        normal: *const f64,
        point: *const f64,
        out: *mut *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn faber_extend_surface(
        input: *const c_void,
        edge: u32,
        distance: f64,
        out: *mut *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn faber_split_surface(
        input: *const c_void,
        tool: *const c_void,
        keep: i32,
        out: *mut *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn faber_surface_piece(
        input: *const c_void,
        index: u32,
        out: *mut *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
    fn faber_replace_surface(
        input: *const c_void,
        index: u32,
        tool: *const c_void,
        out: *mut *mut c_void,
        error: *mut c_char,
        cap: usize,
    ) -> i32;
}

pub(super) fn trim(
    surface: &Shape,
    tool: Option<&Shape>,
    plane: Option<([f64; 3], [f64; 3])>,
    near: [f64; 3],
) -> Result<Shape> {
    let input = pointer(surface)?;
    let mode = if plane.is_none() {
        0
    } else if tool.is_some() {
        1
    } else {
        2
    };
    let raw = tool.map(pointer).transpose()?.unwrap_or(std::ptr::null());
    let (origin, normal) = plane.unwrap_or(([0.; 3], [0., 0., 1.]));
    if !near
        .iter()
        .chain(&origin)
        .chain(&normal)
        .all(|x| x.is_finite())
        || glam::DVec3::from(normal).length_squared() < 1e-20
    {
        return Err("Trim needs finite points and a non-zero normal.".into());
    }
    shape_call(|out, e, n| unsafe {
        faber_trim_surface(
            input,
            raw,
            mode,
            origin.as_ptr(),
            normal.as_ptr(),
            near.as_ptr(),
            out,
            e,
            n,
        )
    })
}

pub(super) fn extend(surface: &Shape, edge: u32, dist: f64) -> Result<Shape> {
    if !dist.is_finite() || dist <= 0. {
        return Err("The extension distance must be positive and finite.".into());
    }
    let input = pointer(surface)?;
    shape_call(|out, e, n| unsafe { faber_extend_surface(input, edge, dist, out, e, n) })
}
pub(super) fn split(solid: &Shape, tool: &Shape, keep: SplitKeep) -> Result<Shape> {
    let (input, cutter) = (pointer(solid)?, pointer(tool)?);
    let side = match keep {
        SplitKeep::Both => 0,
        SplitKeep::Positive => 1,
        SplitKeep::Negative => -1,
    };
    shape_call(|out, e, n| unsafe { faber_split_surface(input, cutter, side, out, e, n) })
}
pub(super) fn piece(solid: &Shape, i: u32) -> Result<Shape> {
    let input = pointer(solid)?;
    shape_call(|out, e, n| unsafe { faber_surface_piece(input, i, out, e, n) })
}
pub(super) fn replace(solid: &Shape, i: u32, tool: &Shape) -> Result<Shape> {
    let (input, cutter) = (pointer(solid)?, pointer(tool)?);
    shape_call(|out, e, n| unsafe { faber_replace_surface(input, i, cutter, out, e, n) })
}
