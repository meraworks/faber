//! Raise or engrave a sketch on one face of a solid.
use faber_model::{EmbossMode, FaceSelector, Placement};
use faber_ports::{Combine, Kernel, Result, Shape};
use glam::DVec3;

fn area(kernel: &dyn Kernel, shape: &Shape) -> Result<f64> {
    Ok(kernel.faces(shape)?.iter().map(|face| face.area).sum())
}

pub(crate) fn build(
    kernel: &dyn Kernel,
    body: &Shape,
    selector: &FaceSelector,
    sketch: &Shape,
    depth: f64,
    mode: EmbossMode,
    at: Option<[f64; 2]>,
) -> Result<Shape> {
    if !depth.is_finite() || depth <= 0. || depth > 1e6 {
        return Err("Emboss depth must be positive and at most 1000000 mm.".into());
    }
    let ids = crate::select::faces(kernel, body, selector)?;
    let [id] = ids.as_slice() else {
        return Err("Select one face for the emboss.".into());
    };
    let face = kernel
        .faces(body)?
        .into_iter()
        .find(|f| f.index == *id)
        .ok_or("The selected face is missing.")?;
    let sketches = kernel.faces(sketch)?;
    if sketches.is_empty() || kernel.properties(sketch)?.solids != 0 {
        return Err("Emboss needs a closed sketch profile.".into());
    }
    let distance = if mode == EmbossMode::Raise {
        depth
    } else {
        -depth
    };
    let target = kernel.face_shape(body, *id)?;
    let (profile, normal) = if let Some(normal) = face.normal {
        let n = DVec3::from(normal);
        let offset = n.dot(DVec3::from(face.centre) - DVec3::from(sketches[0].centre));
        if sketches.iter().any(|f| {
            f.normal.is_none_or(|v| n.dot(v.into()).abs() < 1. - 1e-8)
                || (n.dot(DVec3::from(face.centre) - DVec3::from(f.centre)) - offset).abs() > 1e-6
        }) {
            return Err("The sketch must be parallel to the selected planar face.".into());
        }
        let moved = kernel.transform_shape(
            sketch,
            &Placement {
                translation: (offset * n).to_array(),
                rotation: [0.; 3],
            },
        )?;
        (moved, Some(n))
    } else if let Some(cylinder) = &face.cylinder {
        let radial_depth = if cylinder.internal {
            -distance
        } else {
            distance
        };
        if cylinder.radius + radial_depth <= 1e-7 {
            return Err("The emboss depth reaches the cylinder axis.".into());
        }
        (
            kernel.wrap_on_cylinder(sketch, cylinder, at.unwrap_or_default())?,
            None,
        )
    } else {
        return Err("Emboss needs a flat or a cylindrical face.".into());
    };
    let common = kernel.combine(&target, &profile, Combine::Intersection)?;
    let full_area = area(kernel, &profile)?;
    if full_area <= 1e-10
        || (full_area - area(kernel, &common)?).abs() > 1e-6_f64.max(full_area * 1e-6)
    {
        return Err("The emboss profile extends outside the selected face.".into());
    }
    let tool = if let Some(n) = normal {
        kernel.extrude_shape(&profile, (distance * n).to_array())?
    } else {
        let mut tool = None;
        for f in kernel.faces(&profile)? {
            let wrapped = kernel.face_shape(&profile, f.index)?;
            let patch = kernel.thicken(&wrapped, distance)?;
            tool = Some(match tool {
                Some(previous) => kernel.combine(&previous, &patch, Combine::Union)?,
                None => patch,
            });
        }
        tool.ok_or("The sketch has no closed region.")?
    };
    let operation = if mode == EmbossMode::Raise {
        Combine::Union
    } else {
        Combine::Difference
    };
    kernel.combine(body, &tool, operation)
}
