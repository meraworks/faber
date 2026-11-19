//! Apply solid operations to the material regions of one sketch.
use faber_ports::{Combine, Kernel, Result, Shape};
use glam::DVec3;

struct Region {
    face: Shape,
    centre: DVec3,
    area: f64,
}

fn regions(kernel: &dyn Kernel, source: &Shape) -> Result<Option<Vec<Region>>> {
    let props = kernel.summary_properties(source)?;
    if props.solids > 0 || props.faces < 2 {
        return Ok(None);
    }
    let faces = kernel.faces(source)?;
    let first = faces.first().ok_or("The sketch has no face.")?;
    let origin = DVec3::from(first.centre);
    let normal = first
        .normal
        .map(DVec3::from)
        .ok_or("Sketch regions need a common plane.")?;
    let mut regions = Vec::new();
    for face in faces {
        let centre = DVec3::from(face.centre);
        if face
            .normal
            .is_none_or(|n| normal.dot(n.into()).abs() < 1. - 1e-9)
            || normal.dot(centre - origin).abs() > 1e-6
        {
            return Err("Sketch regions need a common plane.".into());
        }
        regions.push(Region {
            face: kernel.face_shape(source, face.index)?,
            centre,
            area: face.area,
        });
    }
    Ok(Some(regions))
}

fn join(kernel: &dyn Kernel, solids: impl IntoIterator<Item = Result<Shape>>) -> Result<Shape> {
    let mut iter = solids.into_iter();
    let first = iter.next().ok_or("The sketch has no material region.")??;
    iter.try_fold(first, |a, b| kernel.combine(&a, &b?, Combine::Union))
}

pub(crate) fn each(
    kernel: &dyn Kernel,
    source: &Shape,
    op: impl Fn(&Shape) -> Result<Shape>,
) -> Result<Shape> {
    let Some(regions) = regions(kernel, source)? else {
        return op(source);
    };
    join(kernel, regions.iter().map(|r| op(&r.face)))
}

// Relative centres remove the placement and overall spacing of each section.
fn layout(regions: &[Region]) -> Vec<(DVec3, f64)> {
    let origin = regions[0].centre;
    let centre = regions.iter().map(|r| r.centre - origin).sum::<DVec3>() / regions.len() as f64;
    let radius = regions
        .iter()
        .map(|r| (r.centre - origin - centre).length())
        .fold(0., f64::max);
    let scale = if radius > 1e-7 { radius } else { 1. };
    let area: f64 = regions.iter().map(|r| r.area).sum();
    regions
        .iter()
        .map(|r| ((r.centre - origin - centre) / scale, (r.area / area).ln()))
        .collect()
}

fn correspondence(previous: &[Region], next: &[Region]) -> Result<Vec<usize>> {
    if previous.len() != next.len() {
        return Err("Each loft sketch must have the same number of material regions.".into());
    }
    let target = layout(next);
    let mut order = Vec::new();
    let mut used = vec![false; next.len()];
    for (point, area) in layout(previous) {
        let dist = target
            .iter()
            .map(|(q, _)| point.distance(*q))
            .fold(f64::INFINITY, f64::min);
        let mut nearest = target
            .iter()
            .enumerate()
            .filter(|(_, (q, _))| point.distance(*q) <= dist + 1e-8)
            .map(|(i, (_, a))| (i, (a - area).abs()))
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.total_cmp(&b.1));
        let choice = nearest[0];
        if used[choice.0]
            || nearest
                .get(1)
                .is_some_and(|other| other.1 - choice.1 < 1e-8)
        {
            return Err(
                "The loft region match is ambiguous. Use a separate loft for each region.".into(),
            );
        }
        used[choice.0] = true;
        order.push(choice.0);
    }
    Ok(order)
}

pub(crate) fn loft(kernel: &dyn Kernel, sections: &[Shape], ruled: bool) -> Result<Shape> {
    let Some(first) = sections.first() else {
        return kernel.loft_shapes(sections, ruled);
    };
    let Some(mut previous) = regions(kernel, first)? else {
        if sections.iter().skip(1).any(|s| {
            kernel
                .summary_properties(s)
                .is_ok_and(|p| p.solids == 0 && p.faces > 1)
        }) {
            return Err("Each loft sketch must have the same number of material regions.".into());
        }
        return kernel.loft_shapes(sections, ruled);
    };
    let mut tracks: Vec<_> = previous.iter().map(|r| vec![r.face.clone()]).collect();
    for shape in &sections[1..] {
        let unordered = regions(kernel, shape)?
            .ok_or("Each loft sketch must have the same number of material regions.")?;
        let order = correspondence(&previous, &unordered)?;
        let mut items: Vec<_> = unordered.into_iter().map(Some).collect();
        previous = order
            .into_iter()
            .map(|i| items[i].take().unwrap())
            .collect();
        for (track, r) in tracks.iter_mut().zip(&previous) {
            track.push(r.face.clone());
        }
    }
    join(kernel, tracks.iter().map(|s| kernel.loft_shapes(s, ruled)))
}
