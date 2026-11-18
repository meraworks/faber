use faber_model::{CurvePath, CurveSegment, Geometry, LipLoop, LipSide, Placement, RibSide};
use faber_ports::{Combine, Kernel, Result, Shape};
use glam::DVec3;

pub(crate) fn walls(
    kernel: &dyn Kernel,
    body: &Shape,
    plane: &Placement,
    geometry: &Geometry,
) -> Result<Shape> {
    let (lines, thickness, side, draft, web) = match geometry {
        Geometry::Web {
            lines,
            thickness,
            draft_deg,
            ..
        } => (lines, *thickness, RibSide::Symmetric, *draft_deg, true),
        Geometry::Rib {
            lines,
            thickness,
            side,
            draft_deg,
            ..
        } => (lines, *thickness, *side, *draft_deg, false),
        _ => return Err("Select a rib or web.".into()),
    };
    if lines.is_empty() || !thickness.is_finite() || thickness <= 0. {
        return Err("A wall needs at least one line and a positive thickness.".into());
    }
    if draft.is_some_and(|a| !a.is_finite() || a.abs() >= 90.) {
        return Err("The wall draft must be between -90 and 90 degrees.".into());
    }
    let axes = crate::rotation_matrix(plane.rotation);
    let normal = axes.z_axis;
    let offset = match side {
        RibSide::Symmetric => 0.,
        RibSide::Left => 1.,
        RibSide::Right => -1.,
    };
    let mut output = None;
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let [a, b] =
            line.map(|[x, y]| DVec3::from(plane.translation) + axes * DVec3::new(x, y, 0.));
        let tangent = (b - a)
            .try_normalize()
            .ok_or_else(|| format!("Line {number} has zero length."))?;
        let profile = kernel.curve(&CurvePath {
            start: a.into(),
            segments: vec![CurveSegment::Line { end: b.into() }],
        })?;
        let primary = if web { -normal } else { normal.cross(tangent) };
        let grow =
            |d: DVec3, side: f64| kernel.linear_form(body, &profile, d.into(), thickness, side);
        let (mut wall, direction) = match grow(primary, offset) {
            Ok(wall) => (wall, primary),
            Err(error) if error == "open start" || error == "open end" => {
                let kind = if web { "web" } else { "rib" };
                let end = if error == "open start" {
                    "start"
                } else {
                    "end"
                };
                return Err(format!(
                    "The {kind} does not reach the body at the {end} of line {number}."
                ));
            }
            Err(first) => match grow(-primary, -offset) {
                Ok(wall) => (wall, -primary),
                Err(second) => {
                    let error = if second.contains("thickness") {
                        second
                    } else {
                        first
                    };
                    return Err(format!("Line {number}: {error}."));
                }
            },
        };
        if let Some(angle) = draft.filter(|a| *a != 0.) {
            let flank = tangent.cross(primary);
            let distances = [
                (offset - 1.) * thickness / 2.,
                (offset + 1.) * thickness / 2.,
            ];
            let faces = kernel
                .faces(&wall)?
                .into_iter()
                .filter(|f| {
                    f.normal
                        .is_some_and(|n| DVec3::from(n).dot(flank).abs() > 1. - 1e-8)
                        && distances
                            .iter()
                            .any(|d| ((DVec3::from(f.centre) - a).dot(flank) - d).abs() < 1e-5)
                })
                .map(|f| f.index)
                .collect::<Vec<_>>();
            if faces.len() != 2 {
                return Err(format!(
                    "Cannot find the two wall faces for the draft on line {number}."
                ));
            }
            wall = kernel.draft(
                &wall,
                &faces,
                direction.into(),
                a.into(),
                direction.into(),
                angle,
            )?;
        }
        output = Some(match output {
            None => wall,
            Some(previous) => kernel.combine(&previous, &wall, Combine::Union)?,
        });
    }
    let shape = output.ok_or("The wall has no profile.")?;
    let properties = kernel.summary_properties(&shape)?;
    if !properties.valid || properties.solids != 1 {
        return Err("The walls must join the body in one valid solid.".into());
    }
    Ok(shape)
}

/// Make an annulus from two offsets of the same loop.
fn ring(kernel: &dyn Kernel, face: &Shape, index: u32, low: f64, high: f64) -> Result<Shape> {
    let outer = kernel.inset_face(face, index, high)?;
    let inner = kernel.inset_face(face, index, low)?;
    let ring = kernel.combine(&outer, &inner, Combine::Difference)?;
    if kernel.face_count(&ring)? != 1 {
        return Err("The rim width splits or collapses the ring.".into());
    }
    kernel.face_shape(&ring, 1)
}

pub(crate) fn rim(kernel: &dyn Kernel, body: &Shape, geometry: &Geometry) -> Result<Shape> {
    let (selector, loop_, side, width, depth, clearance, cut) = match geometry {
        Geometry::Lip {
            face,
            loop_,
            side,
            width,
            height,
            ..
        } => (face, loop_, side, *width, *height, 0., false),
        Geometry::Groove {
            face,
            loop_,
            side,
            width,
            depth,
            clearance,
            ..
        } => (face, loop_, side, *width, *depth, *clearance, true),
        _ => return Err("Select a lip or groove.".into()),
    };
    if !width.is_finite()
        || width <= 0.
        || !depth.is_finite()
        || depth <= 0.
        || !clearance.is_finite()
        || clearance < 0.
    {
        return Err(
            "Rim width and depth must be positive. Clearance must be zero or positive.".into(),
        );
    }
    let matched = crate::select::faces(kernel, body, selector)?;
    let [id] = matched.as_slice() else {
        return Err("Select one planar face for the rim.".into());
    };
    let face = kernel.face_shape(body, *id)?;
    let info = kernel.faces(&face)?;
    let normal = info
        .first()
        .and_then(|f| f.normal)
        .ok_or("Select a planar face for the rim.")?;
    let index = match loop_ {
        LipLoop::Outer => 0,
        LipLoop::Inner => 1,
    };
    let (low, high) = match side {
        LipSide::Inside => (-width, 0.),
        LipSide::Outside => (0., width),
    };
    let nominal = ring(kernel, &face, index, low, high)?;
    let common = kernel.combine(&face, &nominal, Combine::Intersection)?;
    let area = kernel.faces(&nominal)?.iter().map(|f| f.area).sum::<f64>();
    let supported = kernel.faces(&common)?.iter().map(|f| f.area).sum::<f64>();
    if area <= 0. || supported + 1e-6_f64.max(area * 1e-8) < area {
        return Err(format!(
            "The rim width of {width} mm extends beyond the selected face."
        ));
    }
    let profile = if clearance > 0. {
        ring(kernel, &face, index, low - clearance, high + clearance)?
    } else {
        nominal
    };
    let height = if cut { -(depth + clearance) } else { depth };
    let tool = kernel.extrude_shape(&profile, (DVec3::from(normal) * height).into())?;
    let shape = kernel.combine(
        body,
        &tool,
        if cut {
            Combine::Difference
        } else {
            Combine::Union
        },
    )?;
    let before = kernel.summary_properties(body)?;
    let after = kernel.summary_properties(&shape)?;
    if !after.valid
        || after.solids != 1
        || if cut {
            after.volume >= before.volume
        } else {
            after.volume <= before.volume
        }
    {
        return Err("The rim must change the body and keep one valid solid.".into());
    }
    Ok(shape)
}
