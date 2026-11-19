use super::*;

unsafe extern "C" {
    fn faber_profile_wires(
        src: *const c_void,
        size: usize,
        count: *mut usize,
        out: *mut *mut c_void,
        err: *mut c_char,
        n: usize,
    ) -> i32;
    fn faber_surface_area(src: *const c_void, area: *mut f64, err: *mut c_char, n: usize) -> i32;
    fn faber_free_edges(
        src: *const c_void,
        ids: *mut u32,
        size: usize,
        count: *mut usize,
        err: *mut c_char,
        n: usize,
    ) -> i32;
}
pub(super) fn wires(shape: &Shape) -> Result<Vec<Shape>> {
    let src = pointer(shape)?;
    let mut count = 0usize;
    let guard = gate();
    let mut buffer = [0; 1024];
    error(
        unsafe {
            faber_profile_wires(
                src,
                0,
                &mut count,
                std::ptr::null_mut(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        },
        &buffer,
    )?;
    let mut wires = vec![std::ptr::null_mut(); count];
    error(
        unsafe {
            faber_profile_wires(
                src,
                wires.len(),
                &mut count,
                wires.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        },
        &buffer,
    )?;
    drop(guard);
    Ok(wires
        .into_iter()
        .map(|p| Shape::new(OwnedShape(NonNull::new(p).expect("Each wire has a shape"))))
        .collect())
}
pub(super) fn area(shape: &Shape) -> Result<f64> {
    let src = pointer(shape)?;
    let _guard = gate();
    let mut buffer = [0; 1024];
    let mut area = 0.;
    error(
        unsafe { faber_surface_area(src, &mut area, buffer.as_mut_ptr(), buffer.len()) },
        &buffer,
    )?;
    if area.is_finite() && area > 0. {
        Ok(area)
    } else {
        Err("The surface has no positive area.".into())
    }
}
pub(super) fn open_edges(kernel: &OcctKernel, shape: &Shape) -> Result<Vec<u32>> {
    let n = kernel.summary_properties(shape)?.edges as usize;
    let src = pointer(shape)?;
    let _guard = gate();
    let mut buffer = [0; 1024];
    let mut ids = vec![0; n];
    let mut count = 0;
    error(
        unsafe {
            faber_free_edges(
                src,
                ids.as_mut_ptr(),
                n,
                &mut count,
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        },
        &buffer,
    )?;
    ids.truncate(count);
    Ok(ids)
}
