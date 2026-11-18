use faber_geometry::projected_lines::{ProjectedLines, bounds};
use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{
    Body, Kernel, Mesh, Result, Shape,
    drawing::{Curve2, Projection2, ViewFrame},
};
use glam::{DMat3, DVec3};
use i_overlay::{core::fill_rule::FillRule, float::simplify::SimplifyShape};

fn basis(frame: &ViewFrame) -> Result<DMat3> {
    let x = DVec3::from(frame.x);
    let y = DVec3::from(frame.y);
    if !DVec3::from(frame.origin).is_finite()
        || !x.is_normalized()
        || !y.is_normalized()
        || x.dot(y).abs() > 1e-8
    {
        return Err("A drawing frame needs two perpendicular unit axes.".into());
    }
    Ok(DMat3::from_cols(x, y, x.cross(y)))
}

fn outline(
    mesh: &Mesh,
    axes: DMat3,
    origin: DVec3,
    edge_on: Option<&mut ProjectedLines>,
) -> Vec<Vec<[f64; 2]>> {
    let inverse = axes.transpose();
    let points: Vec<_> = mesh
        .vertices
        .iter()
        .map(|p| (inverse * (DVec3::from(*p) - origin)).truncate())
        .collect();
    let contours: Vec<_> = mesh
        .triangles
        .iter()
        .filter_map(|t| {
            let mut q = t.map(|i| points[i as usize]);
            let area = (q[1] - q[0]).perp_dot(q[2] - q[0]);
            if area.abs() < 1e-16 {
                return None;
            }
            if area < 0. {
                q.swap(1, 2);
            }
            Some(q.map(|v| v.to_array()).to_vec())
        })
        .collect();
    if contours.is_empty() {
        if let Some(lines) = edge_on {
            for t in &mesh.triangles {
                let p = t.map(|i| points[i as usize]);
                let [a, b] = [[p[0], p[1]], [p[0], p[2]], [p[1], p[2]]]
                    .into_iter()
                    .max_by(|a, b| {
                        a[0].distance_squared(a[1])
                            .total_cmp(&b[0].distance_squared(b[1]))
                    })
                    .unwrap();
                lines.add(a.into(), b.into(), false);
            }
        }
        return vec![];
    }
    contours
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect()
}

/// A box in the section frame encloses all material behind the plane.
fn backing_box(bounds: [[f64; 3]; 2], axes: DMat3, origin: DVec3) -> Result<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = -lo;
    for i in 0..8 {
        let p = DVec3::from_array(std::array::from_fn(|a| bounds[(i >> a) & 1][a]));
        let local = axes.transpose() * (p - origin);
        lo = lo.min(local);
        hi = hi.max(local);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err("The projected bounds exceed the coordinate range.".into());
    }
    Ok((lo, hi))
}
fn clipped_solid(
    shape: &Shape,
    lo: DVec3,
    hi: DVec3,
    axes: DMat3,
    origin: DVec3,
) -> Result<Option<Shape>> {
    if lo.z >= -1e-9 {
        return Ok(None);
    }
    if hi.z <= 1e-9 {
        return Ok(Some(shape.clone()));
    }
    let corner = lo - DVec3::ONE;
    let size = DVec3::new(hi.x + 1. - corner.x, hi.y + 1. - corner.y, -corner.z);
    let block = OcctKernel.box_shape(size.into())?;
    let block = OcctKernel.transform_shape(
        &block,
        &Placement {
            translation: (origin + axes * corner).into(),
            rotation: faber_core::euler_from_matrix(axes),
        },
    )?;
    OcctKernel.intersect_solids(shape, &block)
}

pub(super) fn draw(
    bodies: &[&Body],
    frame: &ViewFrame,
    hidden: bool,
    section: bool,
) -> Result<Projection2> {
    let axes = basis(frame)?;
    let origin = frame.origin.into();
    let mut shapes = Vec::new();
    let mut outlines = Vec::new();
    let mut edge_on = ProjectedLines::default();
    for b in bodies {
        let (lo, hi) = backing_box(b.bounds, axes, origin)?;
        if let Some(shape) = &b.shape {
            if section {
                shapes.extend(clipped_solid(shape, lo, hi, axes, origin)?);
            } else {
                shapes.push(shape.clone());
            }
        } else if !section || hi.z <= 1e-9 {
            outlines.extend(outline(&b.mesh, axes, origin, Some(&mut edge_on)));
        } else if lo.z < 0. {
            let normal = axes.z_axis;
            if b.mesh
                .triangle_vertices()
                .any(|p| normal.dot(DVec3::from(p) - origin) < 0.)
            {
                let sides = faber_meshtools::cut(
                    &b.mesh,
                    frame.origin,
                    normal.into(),
                    faber_meshtools::Keep::Below,
                    false,
                )?;
                for side in sides {
                    outlines.extend(outline(&side, axes, origin, Some(&mut edge_on)));
                }
            }
        }
    }
    let mut result = OcctKernel.project_edges(&shapes, frame, hidden)?;
    edge_on.append_to(&mut result);
    for mut contour in outlines {
        if let Some(&p) = contour.first() {
            contour.push(p);
            result.visible.push(Curve2::Polyline(contour));
        }
    }
    result.bounds = bounds(&result);
    Ok(result)
}

pub(super) fn section(bodies: &[&Body], frame: &ViewFrame) -> Result<Vec<Vec<[f64; 2]>>> {
    let axes = basis(frame)?;
    let origin = frame.origin.into();
    let mut loops = vec![];
    for b in bodies {
        let Some(shape) = &b.shape else {
            continue;
        };
        let (lo, hi) = backing_box(b.bounds, axes, origin)?;
        if lo.z > 1e-8 || hi.z < -1e-8 {
            continue;
        }
        let faces = OcctKernel.section_face(shape, frame.origin, axes.z_axis.into())?;
        if OcctKernel.face_count(&faces)? > 0 {
            let mesh = OcctKernel.tessellate(&faces, 0.01)?;
            loops.extend(outline(&mesh, axes, origin, None));
        }
    }
    if loops.len() <= 1 {
        return Ok(loops);
    }
    Ok(loops
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect())
}
