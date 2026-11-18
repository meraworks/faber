//! Print checks use placed parts and world directions.
use faber_core::{Project, euler_from_matrix, resolved_placements, rotation_matrix};
use faber_kernel_occt::OcctKernel;
use faber_model::{Edit, Placement, Printer, ProjectData, Scalar};
use faber_ports::{Body, Kernel, Mesh, Result};
pub use faber_print::{Candidate, CylinderFit, PrintReport, StackItem, checked_stack};
use glam::DVec3;
use std::borrow::Cow;
use std::collections::BTreeSet;

pub fn printer(data: &ProjectData) -> Printer {
    data.printer.clone().unwrap_or_default()
}

fn body<'a>(project: &'a Project, id: &str) -> Result<&'a Body> {
    project
        .bodies
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| format!("Unknown part: {id}"))
}

pub fn print_check(project: &Project, ids: Option<&[String]>, up: [f64; 3]) -> Result<PrintReport> {
    print_check_at(project, ids, up, None)
}

fn analysis_mesh(body: &Body, resolution_mm: Option<f64>) -> Result<Cow<'_, Mesh>> {
    if let Some(mm) = resolution_mm {
        if !(0.001..=1.).contains(&mm) {
            return Err(format!("Resolution {mm} mm is outside 0.001 to 1.0 mm."));
        }
        if let Some(shape) = &body.shape {
            return OcctKernel.tessellate(shape, mm).map(Cow::Owned);
        }
    }
    Ok(Cow::Borrowed(&body.mesh))
}

pub fn print_check_at(
    project: &Project,
    ids: Option<&[String]>,
    up: [f64; 3],
    resolution_mm: Option<f64>,
) -> Result<PrintReport> {
    let parts = match ids {
        None => project.bodies.iter().collect(),
        Some(ids) => {
            let mut used = BTreeSet::new();
            ids.iter()
                .map(|id| {
                    if !used.insert(id) {
                        return Err(format!("Part listed twice: {id}"));
                    }
                    body(project, id)
                })
                .collect::<Result<Vec<_>>>()?
        }
    };
    let key = serde_json::to_string(&(ids, up.map(f64::to_bits), resolution_mm.map(f64::to_bits)))
        .map_err(|e| e.to_string())?;
    project.cached("print_check", &key, || {
        let meshes = parts
            .into_iter()
            .map(|part| analysis_mesh(part, resolution_mm))
            .collect::<Result<Vec<_>>>()?;
        let borrowed = meshes.iter().map(Cow::as_ref).collect::<Vec<_>>();
        faber_print::check(&borrowed, &printer(&project.data), up)
    })
}

pub fn orientations(
    project: &Project,
    id: &str,
    strength_direction: Option<[f64; 3]>,
) -> Result<Vec<Candidate>> {
    orientations_at(project, id, strength_direction, None)
}

pub fn orientations_at(
    project: &Project,
    id: &str,
    strength_direction: Option<[f64; 3]>,
    resolution_mm: Option<f64>,
) -> Result<Vec<Candidate>> {
    let part = body(project, id)?;
    let key = serde_json::to_string(&(
        id,
        strength_direction.map(|v| v.map(f64::to_bits)),
        resolution_mm.map(f64::to_bits),
    ))
    .map_err(|e| e.to_string())?;
    project.cached("orient_for_print", &key, || {
        faber_print::rank(
            analysis_mesh(part, resolution_mm)?.as_ref(),
            &printer(&project.data),
            faber_print::Weights { strength_direction },
        )
    })
}

// Keep the effect of future parameter edits on each placement coordinate.
fn offset_coordinate(original: &Scalar, current: f64, target: f64) -> Scalar {
    match original {
        Scalar::Number(_) => Scalar::Number(target),
        Scalar::Expression(_) if target == current => original.clone(),
        Scalar::Expression(expression) => {
            Scalar::Expression(format!("({expression}) + ({})", target - current))
        }
    }
}

pub fn apply_orientation(project: &mut Project, id: &str, candidate: &Candidate) -> Result<()> {
    if !candidate.fits {
        return Err("This orientation exceeds the printer volume.".into());
    }
    let revision = project.data.revision;
    let body = body(project, id)?;
    let mut part = project
        .data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?
        .clone();
    let current = resolved_placements(&project.data)?
        .remove(id)
        .ok_or("Part placement is missing.")?;
    let turn = rotation_matrix(candidate.rotation_deg);
    let centre = (DVec3::from(body.bounds[0]) + DVec3::from(body.bounds[1])) / 2.;
    let mut shift = centre - turn * centre;
    let low = if let Some(shape) = &body.shape {
        let shape = OcctKernel.transform_shape(
            shape,
            &Placement {
                rotation: candidate.rotation_deg,
                translation: shift.to_array(),
            },
        )?;
        OcctKernel.properties(&shape)?.bounds[0][2]
    } else {
        body.mesh
            .triangle_vertices()
            .map(|v| (turn * DVec3::from(v) + shift).z)
            .fold(f64::INFINITY, f64::min)
    };
    shift.z -= low;
    let rotation = euler_from_matrix(turn * rotation_matrix(current.rotation));
    let translation = (turn * DVec3::from(current.translation) + shift).to_array();
    part.placement = Placement {
        rotation: std::array::from_fn(|i| {
            offset_coordinate(
                &part.placement.rotation[i],
                current.rotation[i],
                rotation[i],
            )
        }),
        translation: std::array::from_fn(|i| {
            offset_coordinate(
                &part.placement.translation[i],
                current.translation[i],
                translation[i],
            )
        }),
    };
    project.batch_at_revision(
        revision,
        vec![Edit::PutPart {
            id: id.into(),
            part,
        }],
    )
}

pub fn fit_check(project: &Project, a: &str, b: &str) -> Result<Vec<CylinderFit>> {
    if a == b {
        return Err("Select two different parts for a fit check.".into());
    }
    let key = serde_json::to_string(&(a, b)).map_err(|e| e.to_string())?;
    project.cached("fit_check", &key, || {
        let faces_a = project.topology(a)?.1;
        let faces_b = project.topology(b)?.1;
        Ok(faber_print::cylinder_fits(
            &faces_a,
            &faces_b,
            &printer(&project.data).clearance_mm,
        ))
    })
}

pub(super) fn export_assembly(
    bodies: &[Body],
    frames: &std::collections::BTreeMap<String, Placement>,
    directory: &std::path::Path,
    tolerance: f64,
) -> Result<Vec<super::ExportFile>> {
    use faber_ports::{MeshAssemblyExport, MeshItem};
    let mut meshes = Vec::with_capacity(bodies.len());
    let mut transforms = Vec::with_capacity(bodies.len());
    for body in bodies {
        let frame = frames.get(&body.id).cloned().unwrap_or_default();
        let axes = rotation_matrix(frame.rotation);
        let origin = DVec3::from(frame.translation);
        let mut mesh = match &body.shape {
            Some(shape) => OcctKernel.tessellate(shape, tolerance)?,
            None => body.mesh.clone(),
        };
        for p in &mut mesh.vertices {
            *p = (axes.transpose() * (DVec3::from(*p) - origin)).into();
        }
        let mut transform = [0.; 12];
        transform[..9].copy_from_slice(&axes.to_cols_array());
        transform[9..].copy_from_slice(&frame.translation);
        transforms.push(transform);
        meshes.push(mesh);
    }
    let items: Vec<_> = bodies
        .iter()
        .zip(&meshes)
        .zip(transforms)
        .map(|((body, mesh), transform)| MeshItem {
            name: &body.name,
            colour: body.colour,
            mesh,
            transform,
        })
        .collect();
    let path = directory.join("assembly.3mf");
    let triangles = faber_files::ThreeMf.write_assembly(&items, &path)?;
    Ok(vec![super::ExportFile {
        path: path.to_string_lossy().into_owned(),
        triangles: Some(triangles),
    }])
}
