use super::{error, gate, pointer, shape_call};
use faber_geometry::projected_lines::{ProjectedLines, bounds};
use faber_ports::{
    Result, Shape,
    drawing::{Curve2, Projection2, ViewFrame},
};
use glam::{DVec2, DVec3};
use std::{
    collections::BTreeSet,
    ffi::{c_char, c_void},
    ptr, slice,
};

#[repr(C)]
struct RawCurve {
    kind: i32,
    hidden: i32,
    offset: usize,
    count: usize,
    data: [f64; 5],
}
#[repr(C)]
struct RawDrawing {
    owner: *mut c_void,
    curves: *const RawCurve,
    curve_count: usize,
    points: *const [f64; 2],
    point_count: usize,
}
unsafe extern "C" {
    fn faber_project_edges(
        inputs: *const *const c_void,
        count: usize,
        origin: *const f64,
        x: *const f64,
        y: *const f64,
        hidden: i32,
        result: *mut RawDrawing,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_free_drawing(owner: *mut c_void);
    fn faber_section_face(
        input: *const c_void,
        origin: *const f64,
        normal: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
}
impl Drop for RawDrawing {
    fn drop(&mut self) {
        unsafe { faber_free_drawing(self.owner) };
    }
}

pub(super) fn section(shape: &Shape, origin: [f64; 3], normal: [f64; 3]) -> Result<Shape> {
    let n = DVec3::from(normal)
        .try_normalize()
        .ok_or("The section normal must be finite and non-zero.")?;
    if !DVec3::from(origin).is_finite() {
        return Err("The section origin must be finite.".into());
    }
    let raw = pointer(shape)?;
    shape_call(|out, err, len| unsafe {
        faber_section_face(raw, origin.as_ptr(), n.to_array().as_ptr(), out, err, len)
    })
}

pub(super) fn project(shapes: &[Shape], frame: &ViewFrame, hidden: bool) -> Result<Projection2> {
    let (x, y) = (DVec3::from(frame.x), DVec3::from(frame.y));
    if !DVec3::from(frame.origin).is_finite()
        || !x.is_normalized()
        || !y.is_normalized()
        || x.dot(y).abs() > 1e-8
    {
        return Err("The drawing frame needs an origin and two perpendicular unit axes.".into());
    }
    if shapes.is_empty() {
        return Ok(Projection2::default());
    }
    let shapes = shapes.iter().map(pointer).collect::<Result<Vec<_>>>()?;
    let mut result = RawDrawing {
        owner: ptr::null_mut(),
        curves: ptr::null(),
        curve_count: 0,
        points: ptr::null(),
        point_count: 0,
    };
    let mut buffer = [0; 1024];
    let guard = gate();
    error(
        unsafe {
            faber_project_edges(
                shapes.as_ptr(),
                shapes.len(),
                frame.origin.as_ptr(),
                frame.x.as_ptr(),
                frame.y.as_ptr(),
                i32::from(hidden),
                &mut result,
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        },
        &buffer,
    )?;
    drop(guard);
    let curves = if result.curve_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(result.curves, result.curve_count) }
    };
    let points = if result.point_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(result.points, result.point_count) }
    };
    let mut output = Projection2::default();
    let mut groups = ProjectedLines::default();
    let mut circles = BTreeSet::new();
    for raw in curves {
        let [cx, cy, radius, start_deg, end_deg] = raw.data;
        if !raw.data.iter().all(|v| v.is_finite()) {
            return Err("The projected curve exceeds the coordinate range.".into());
        }
        let centre = [cx, cy];
        let p = points
            .get(
                raw.offset
                    ..raw
                        .offset
                        .checked_add(raw.count)
                        .ok_or("Invalid curve point count.")?,
            )
            .ok_or("Invalid projection point range.")?;
        if p.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid projected coordinate.".into());
        }
        let visibility = usize::from(raw.hidden != 0);
        let c = match raw.kind {
            0 => {
                groups.add(p[0], p[1], raw.hidden != 0);
                continue;
            }
            1 | 2 => {
                let key = [quant(cx), quant(cy), quant(radius)];
                let hidden = raw.hidden != 0;
                if circles.contains(&(key, 0, 0, false)) || circles.contains(&(key, 0, 0, hidden)) {
                    continue;
                }
                if raw.kind == 1 {
                    circles.insert((key, 0, 0, hidden));
                    Curve2::Circle { centre, radius }
                } else {
                    let a = quant(start_deg.rem_euclid(360.));
                    let span = quant(end_deg - start_deg);
                    if circles.contains(&(key, a, span, false))
                        || !circles.insert((key, a, span, hidden))
                    {
                        continue;
                    }
                    Curve2::Arc {
                        centre,
                        radius,
                        start_deg,
                        end_deg,
                    }
                }
            }
            3 => {
                if p.len() < 2 {
                    continue;
                }
                let a = DVec2::from(p[0]);
                let b = DVec2::from(*p.last().unwrap());
                if let Some(dir) = (b - a).try_normalize()
                    && p.iter()
                        .all(|v| dir.perp_dot(DVec2::from(*v) - a).abs() < 1e-8)
                {
                    let (mut min, mut max) = (0_f64, 0_f64);
                    for q in p {
                        let t = (DVec2::from(*q) - a).dot(dir);
                        min = min.min(t);
                        max = max.max(t);
                    }
                    groups.add(
                        (a + min * dir).into(),
                        (a + max * dir).into(),
                        raw.hidden != 0,
                    );
                    continue;
                }
                Curve2::Polyline(p.to_vec())
            }
            _ => return Err("Unknown projected curve.".into()),
        };
        if visibility == 0 {
            output.visible.push(c)
        } else {
            output.hidden.push(c)
        }
    }
    groups.append_to(&mut output);
    output.bounds = bounds(&output);
    Ok(output)
}

fn quant(n: f64) -> i64 {
    (n * 1e7).round() as i64
}
