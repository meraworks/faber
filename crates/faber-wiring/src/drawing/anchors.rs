use super::DrawingModel;
use faber_core::{Project, rotation_matrix};
use faber_drawing::Resolved;
use faber_kernel_occt::OcctKernel;
use faber_model::{Placement, drawing::Anchor};
use faber_ports::{Body, CylinderFace, FaceInfo, Kernel, Result};
use glam::{DMat3, DVec3};
use std::{collections::BTreeSet, sync::Arc};

fn placement(project: &Project, id: &str) -> Result<(DMat3, DVec3)> {
    project.cached("drawing_placement", id, || {
        let place = &faber_core::resolved_placements(&project.data)?[id];
        Ok((
            rotation_matrix(place.rotation),
            DVec3::from(place.translation),
        ))
    })
}

/// Model vertices and support points on smooth faces, in assembled coordinates.
fn points(project: &Project, body: &Body) -> Result<Arc<Vec<[f64; 3]>>> {
    project.cached("drawing_vertices", &body.id, || {
        let mut pts = Vec::new();
        let mut seen = BTreeSet::new();
        let mut add = |v: [f64; 3]| {
            if seen.insert(v.map(|n| (n * 1e8).round() as i64)) {
                pts.push(v);
            }
        };
        if body.shape.is_some() {
            for range in &body.edge_lines.ranges {
                if let Some(slice) = body
                    .edge_lines
                    .points
                    .get(range.start as usize..range.end as usize)
                    && let (Some(&start), Some(&end)) = (slice.first(), slice.last())
                {
                    add(start);
                    add(end);
                }
            }
        } else {
            for p in body.mesh.triangle_vertices() {
                add(p);
            }
        }
        if let Some(shape) = &body.shape {
            let (lo, hi) = (DVec3::from(body.bounds[0]), DVec3::from(body.bounds[1]));
            for side in 0..2 {
                for axis in 0..3 {
                    let extremum = body.bounds[side][axis];
                    if pts.iter().any(|p| (p[axis] - extremum).abs() < 1e-6) {
                        continue;
                    }
                    // A parallel plate outside the bounds gives a true support point on the solid.
                    let mut size = hi - lo + DVec3::splat(2.);
                    size[axis] = 1.;
                    let mut corner = lo - DVec3::ONE;
                    corner[axis] = if side == 0 {
                        lo[axis] - 2.
                    } else {
                        hi[axis] + 1.
                    };
                    let plate = OcctKernel.box_shape(size.into())?;
                    let plate = OcctKernel.transform_shape(
                        &plate,
                        &Placement {
                            translation: corner.into(),
                            ..Default::default()
                        },
                    )?;
                    let measured = OcctKernel.distance(shape, &plate)?;
                    pts.push(measured.point_a);
                }
            }
        }
        Ok(Arc::new(pts))
    })
}

fn axis_of(c: &CylinderFace, r: DMat3, t: DVec3) -> Resolved {
    let axis = DVec3::from(c.axis);
    let centre = DVec3::from(c.start) + 0.5 * c.length * axis;
    Resolved::Circle {
        centre: (r * centre + t).into(),
        axis: (r * axis).into(),
        radius: c.radius,
    }
}

fn faces(project: &Project, id: &str) -> Result<Arc<Vec<FaceInfo>>> {
    project.cached("drawing_faces", id, || {
        let shape = project
            .local_geometry(id)?
            .1
            .ok_or("The part has no exact faces.")?;
        Ok(Arc::new(OcctKernel.faces(shape)?))
    })
}

pub(super) fn resolve(model: &DrawingModel<'_>, anchor: &Anchor) -> Result<Resolved> {
    let project = model.project;
    match anchor {
        Anchor::Vertex { part, near } => {
            let body = model.body(part)?;
            let (r, t) = placement(project, part)?;
            let query = r * DVec3::from(*near) + t;
            let candidates = points(project, body)?;
            let p = candidates
                .iter()
                .min_by(|&&a, &&b| {
                    query
                        .distance_squared(a.into())
                        .total_cmp(&query.distance_squared(b.into()))
                })
                .ok_or("The vertex reference was not found.")?;
            let diagonal = DVec3::from(body.bounds[1]) - DVec3::from(body.bounds[0]);
            if query.distance(DVec3::from(*p)) > diagonal.length() + 1e-6 {
                return Err("The vertex reference was not found.".into());
            }
            Ok(Resolved::Point(*p))
        }
        Anchor::CylinderAxis { part, face } => {
            let id = project
                .selected_local_face(part, face)
                .map_err(|_| "The cylindrical face reference was not found.")?;
            let list = faces(project, part)?;
            let c = list
                .iter()
                .find(|f| f.index == id)
                .and_then(|f| f.cylinder.as_ref())
                .ok_or("The face is not cylindrical.")?;
            let (r, t) = placement(project, part)?;
            Ok(axis_of(c, r, t))
        }
        Anchor::Edge { part, edge } => {
            let index = project
                .selected_local_edge(part, edge)
                .map_err(|_| "The edge reference was not found.")?;
            let body = model.body(part)?;
            let shape = body.shape.as_ref().ok_or("The part has no exact edges.")?;
            let description = OcctKernel
                .edges(shape)?
                .into_iter()
                .find(|e| e.index == index)
                .ok_or("The edge reference was not found.")?;
            let samples = body
                .edge_lines
                .edge(index)
                .ok_or("The edge has no sample points.")?;
            let first = *samples.first().ok_or("The edge is degenerate.")?;
            let last = *samples.last().unwrap();
            if let Some(radius) = description.radius {
                let a = DVec3::from(first);
                let b = samples
                    .iter()
                    .map(|&p| DVec3::from(p) - a)
                    .max_by(|x, y| x.length_squared().total_cmp(&y.length_squared()))
                    .unwrap();
                let c = samples
                    .iter()
                    .map(|&p| DVec3::from(p) - a)
                    .max_by(|x, y| {
                        b.cross(*x)
                            .length_squared()
                            .total_cmp(&b.cross(*y).length_squared())
                    })
                    .unwrap();
                let normal = b.cross(c);
                let axis = normal
                    .try_normalize()
                    .ok_or("The circular edge is too short to measure.")?;
                let centre = a
                    + (b.length_squared() * c.cross(normal) + c.length_squared() * normal.cross(b))
                        / (2. * normal.length_squared());
                Ok(Resolved::Circle {
                    centre: centre.into(),
                    axis: axis.into(),
                    radius,
                })
            } else if description.direction.is_some() {
                Ok(Resolved::Line([first, last]))
            } else {
                Ok(Resolved::Point(description.centre))
            }
        }
    }
}

pub(super) fn enumerate(
    model: &DrawingModel<'_>,
    bodies: &[&Body],
) -> Result<Vec<(Anchor, Resolved)>> {
    let mut result = vec![];
    for body in bodies {
        let project = model.project;
        let id = &body.id;
        let (r, t) = placement(project, id)?;
        let pts = points(project, body)?;
        let mut used = BTreeSet::new();
        for axis in 0..3 {
            for side in 0..2 {
                let line = body.bounds[side][axis];
                if let Some((i, &p)) = pts.iter().enumerate().min_by(|(_, a), (_, b)| {
                    (a[axis] - line).abs().total_cmp(&(b[axis] - line).abs())
                }) && used.insert(i)
                {
                    let near = (r.transpose() * (DVec3::from(p) - t)).into();
                    result.push((
                        Anchor::Vertex {
                            part: id.clone(),
                            near,
                        },
                        Resolved::Point(p),
                    ));
                }
            }
        }
        if body.shape.is_none() {
            continue;
        }
        let list = faces(project, id)?;
        for f in list.iter() {
            let Some(c) = &f.cylinder else {
                continue;
            };
            let full_area = std::f64::consts::TAU * c.radius * c.length;
            if (full_area - f.area).abs() > 1e-5 * full_area {
                continue;
            }
            let mut selector = faber_core::pick_selector::face_selector(&list, &[f.index])?
                .map_numbers(&mut |s| match s {
                    faber_model::Scalar::Number(n) => Ok(*n),
                    _ => Err("The automatic face selector needs a numeric value.".into()),
                })?;
            selector.internal = Some(c.internal);
            result.push((
                Anchor::CylinderAxis {
                    part: id.clone(),
                    face: Box::new(selector),
                },
                axis_of(c, r, t),
            ));
        }
    }
    Ok(result)
}
