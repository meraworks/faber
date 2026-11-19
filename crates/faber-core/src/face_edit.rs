//! Tools for exact face changes in a feature history.
use faber_model::{FaceSelector, Placement, SketchLoop, SketchProfile};
use faber_ports::{Kernel, Result, Shape};
use glam::DVec3;
use std::collections::BTreeSet;

pub(super) fn selected(
    kernel: &dyn Kernel,
    body: &Shape,
    selectors: &[FaceSelector],
) -> Result<Vec<u32>> {
    let mut indices = BTreeSet::new();
    for selector in selectors {
        for index in crate::select::faces(kernel, body, selector)? {
            if !indices.insert(index) {
                return Err(format!("Face {index} was selected twice."));
            }
        }
    }
    if indices.is_empty() {
        return Err("Select at least one face.".into());
    }
    Ok(indices.into_iter().collect())
}
fn corners(bounds: [[f64; 3]; 2]) -> impl Iterator<Item = DVec3> {
    (0..8).map(move |i| {
        DVec3::new(
            bounds[i & 1][0],
            bounds[(i >> 1) & 1][1],
            bounds[(i >> 2) & 1][2],
        )
    })
}
pub(super) fn plane_tool(kernel: &dyn Kernel, body: &Shape, plane: &Placement) -> Result<Shape> {
    let bounds = kernel.properties(body)?.bounds;
    let inverse = crate::rotation_matrix(plane.rotation).transpose();
    let origin = DVec3::from(plane.translation);
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for corner in corners(bounds) {
        let p = inverse * (corner - origin);
        min = min.min(p);
        max = max.max(p);
    }
    kernel.sketch(
        &SketchProfile {
            outer: SketchLoop::Rectangle {
                origin: [min.x - 1., min.y - 1.],
                size: [max.x - min.x + 2., max.y - min.y + 2.],
            },
            holes: vec![],
        },
        plane,
    )
}
pub(super) fn sketch_tool(kernel: &dyn Kernel, body: &Shape, sketch: &Shape) -> Result<Shape> {
    let faces = kernel.faces(sketch)?;
    let first = faces
        .first()
        .ok_or("The split tool needs a planar sketch.")?;
    let n = DVec3::from(
        first
            .normal
            .ok_or("The split tool needs a planar sketch.")?,
    );
    let origin = DVec3::from(first.centre);
    if kernel.summary_properties(sketch)?.solids != 0
        || faces.iter().any(|f| {
            f.normal
                .is_none_or(|v| DVec3::from(v).cross(n).length() > 1e-7)
                || (DVec3::from(f.centre) - origin).dot(n).abs() > 1e-6
        })
    {
        return Err("The split tool needs a planar sketch.".into());
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for c in corners(kernel.properties(body)?.bounds) {
        let d = (c - origin).dot(n);
        low = low.min(d);
        high = high.max(d);
    }
    let translated = kernel.transform_shape(
        sketch,
        &Placement {
            translation: (n * (low - 1.)).to_array(),
            rotation: [0.; 3],
        },
    )?;
    kernel.extrude_shape(&translated, (n * (high - low + 2.)).to_array())
}
