use super::Built;
use faber_model::{EdgeRef, Feature, Geometry, PathInput};
use faber_ports::{Combine, Kernel, Result, Shape};
use glam::DVec3;

pub fn creates_surface<N>(geometry: &Geometry<N>) -> bool {
    matches!(
        geometry,
        Geometry::SurfaceExtrude { .. }
            | Geometry::SurfaceRevolve { .. }
            | Geometry::SurfaceLoft { .. }
            | Geometry::SurfaceSweep { .. }
            | Geometry::Patch { .. }
            | Geometry::Trim { .. }
            | Geometry::Extend { .. }
            | Geometry::Stitch {
                as_solid: false,
                ..
            }
    )
}

pub(super) fn is_operation<N>(geometry: &Geometry<N>) -> bool {
    creates_surface(geometry)
        || matches!(
            geometry,
            Geometry::Stitch { .. } | Geometry::SplitBody { .. } | Geometry::ReplaceFace { .. }
        )
}

/// Follow placements and suppressed modifiers to the operation that made the shape.
pub fn source_geometry<'a, N>(features: &'a [Feature<N>], id: &str) -> Option<&'a Geometry<N>> {
    let mut name = id;
    for _ in 0..features.len() {
        let entry = features.iter().find(|entry| entry.id == name)?;
        if entry.suppressed {
            name = super::primary_input(&entry.geometry)?;
        } else {
            match &entry.geometry {
                Geometry::Transform { source, .. }
                | Geometry::PlaceAtPoint { source, .. }
                | Geometry::Scale { source, .. } => name = source,
                geometry => return Some(geometry),
            }
        }
    }
    None
}

pub fn is_surface_output<N>(features: &[Feature<N>], id: &str) -> bool {
    source_geometry(features, id).is_some_and(creates_surface)
}

fn joined(kernel: &dyn Kernel, shapes: impl IntoIterator<Item = Result<Shape>>) -> Result<Shape> {
    let mut iter = shapes.into_iter();
    let first = iter.next().ok_or("The profile has no wires.")??;
    iter.try_fold(first, |left, right| {
        kernel.combine(&left, &right?, Combine::Union)
    })
}

fn one(indices: &[u32]) -> Result<u32> {
    match indices {
        [index] => Ok(*index),
        _ => Err("Select one item.".into()),
    }
}

pub(super) fn build(
    kernel: &dyn Kernel,
    geometry: &Geometry,
    features: &[Feature],
    deps: &[Built],
) -> Result<Shape> {
    let shape = |index: usize| match deps.get(index) {
        Some(Built::Shape(s)) => Ok(s),
        Some(Built::OpenSketch(profile)) => Err(profile.reason.clone()),
        _ => Err("The operation needs a shape.".into()),
    };
    let profile_wires = |index: usize| match deps.get(index) {
        Some(Built::OpenSketch(profile)) => profile.wires(kernel),
        _ => kernel.profile_wires(shape(index)?),
    };
    match geometry {
        Geometry::SurfaceExtrude {
            direction,
            distance,
            ..
        } => {
            let direction = match direction {
                Some(axis) => *axis,
                None => match deps.first() {
                    Some(Built::OpenSketch(profile)) => {
                        crate::construction::plane_of(&profile.plane).1.to_array()
                    }
                    _ => kernel
                        .faces(shape(0)?)?
                        .first()
                        .and_then(|face| face.normal)
                        .unwrap_or([0., 0., 1.]),
                },
            };
            let direction = DVec3::from(direction)
                .try_normalize()
                .ok_or("Select a non-zero extrusion direction.")?;
            let vector = (direction * *distance).to_array();
            joined(
                kernel,
                profile_wires(0)?
                    .iter()
                    .map(|wire| kernel.extrude_shape(wire, vector)),
            )
        }
        Geometry::SurfaceRevolve { angle_deg, .. } => {
            let Some(Built::Axis { origin, direction }) = deps.get(1) else {
                return Err("Select a reference axis.".into());
            };
            let axis = direction.map(|x| x * angle_deg.signum());
            joined(
                kernel,
                profile_wires(0)?
                    .iter()
                    .map(|wire| kernel.revolve_shape(wire, *origin, axis, angle_deg.abs())),
            )
        }
        Geometry::SurfaceLoft { ruled, .. } => {
            if !(2..=256).contains(&deps.len()) {
                return Err("A loft needs between 2 and 256 sections.".into());
            }
            let sections = (0..deps.len())
                .map(profile_wires)
                .collect::<Result<Vec<_>>>()?;
            let count = sections[0].len();
            if sections.iter().any(|section| section.len() != count) {
                return Err("Each loft section needs the same number of wires.".into());
            }
            joined(
                kernel,
                (0..count).map(|index| {
                    let wires = sections
                        .iter()
                        .map(|section| section[index].clone())
                        .collect::<Vec<_>>();
                    kernel.loft_shapes(&wires, *ruled)
                }),
            )
        }
        Geometry::SurfaceSweep { path, .. } => {
            let path = match path {
                PathInput::Curve(curve) => kernel.curve(curve)?,
                PathInput::Feature(_) if matches!(deps.get(1), Some(Built::OpenSketch(_))) => {
                    let paths = profile_wires(1)?;
                    match paths.as_slice() {
                        [wire] => wire.clone(),
                        _ => return Err("A sweep path needs one continuous chain.".into()),
                    }
                }
                PathInput::Feature(_) => shape(1)?.clone(),
            };
            joined(
                kernel,
                profile_wires(0)?
                    .iter()
                    .map(|wire| kernel.sweep_shape(wire, &path, false)),
            )
        }
        Geometry::Patch { boundary, .. } => {
            let mut wires = Vec::new();
            for (index, item) in boundary.iter().enumerate() {
                match item {
                    EdgeRef::Edge { edge, .. } => {
                        let input = shape(index)?;
                        for edge in crate::select::edges(kernel, input, edge)? {
                            wires.push(kernel.edge_shape(input, edge)?)
                        }
                    }
                    EdgeRef::Curve { .. } => wires.extend(profile_wires(index)?),
                }
            }
            let support = (boundary.len()..deps.len())
                .map(|i| shape(i).cloned())
                .collect::<Result<Vec<_>>>()?;
            kernel.patch(&wires, &support)
        }
        Geometry::Stitch {
            tolerance,
            as_solid,
            ..
        } => {
            let inputs = (0..deps.len())
                .map(|i| shape(i).cloned())
                .collect::<Result<Vec<_>>>()?;
            let result = kernel.stitch(&inputs, tolerance.unwrap_or(0.001), *as_solid)?;
            Ok(result.shape.with_stitch_gap(result.largest_gap))
        }
        Geometry::Trim { tool, keep, .. } => {
            let surface = shape(0)?;
            if let Some(Built::Plane(plane)) = deps.get(1) {
                let (origin, normal) = crate::construction::plane_of(plane);
                return kernel.trim_with_plane(
                    surface,
                    origin.to_array(),
                    normal.to_array(),
                    keep.near,
                );
            }
            let cutter = shape(1)?;
            if matches!(
                source_geometry(features, tool),
                Some(
                    Geometry::Sketch { .. }
                        | Geometry::SketchOnPlane { .. }
                        | Geometry::SolvedSketch { .. }
                        | Geometry::OffsetSketch { .. }
                )
            ) {
                let normal = kernel
                    .faces(cutter)?
                    .first()
                    .and_then(|face| face.normal)
                    .ok_or("The sketch needs a plane.")?;
                kernel.trim_with_sketch(surface, cutter, normal, keep.near)
            } else {
                kernel.trim_surface(surface, cutter, keep.near)
            }
        }
        Geometry::Extend { edge, distance, .. } => {
            let surface = shape(0)?;
            kernel.extend_surface(
                surface,
                one(&crate::select::edges(kernel, surface, edge)?)?,
                *distance,
            )
        }
        Geometry::SplitBody { keep, .. } => {
            kernel.split_body_by_surface(shape(0)?, shape(1)?, *keep)
        }
        Geometry::ReplaceFace { face, .. } => {
            let body = shape(0)?;
            kernel.replace_face(
                body,
                one(&crate::select::faces(kernel, body, face)?)?,
                shape(1)?,
            )
        }
        _ => Err("Select a surface operation.".into()),
    }
}
