//! Edit an imported mesh in its part frame. Each change adds one undo step.

use faber_core::Project;
use faber_kernel_occt::OcctKernel;
pub use faber_meshtools::{Keep, MeshReport, RepairOptions, Target};
use faber_model::{Edit, Feature, Geometry, Scalar};
use faber_ports::{Kernel, Result};
use glam::DVec3;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum MeshEdit {
    Repair(RepairOptions),
    Reduce(Target),
    Cut {
        origin: [f64; 3],
        normal: [f64; 3],
        keep: Keep,
        cap: bool,
        copy_as: Option<String>,
    },
    ToSolid {
        merge_planar: bool,
        copy_as: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct EditResult {
    pub changed: bool,
    pub revision: u64,
    pub parts: Vec<String>,
    pub log: Value,
}

pub fn report(project: &Project, part: &str) -> Result<MeshReport> {
    project.cached("mesh_report", part, || {
        faber_meshtools::try_report(project.imported_mesh(part)?)
    })
}

/// World coordinates for cut planes. A split keeps the lower side in the original part.
pub fn apply(
    project: &mut Project,
    part: &str,
    revision: u64,
    operation: MeshEdit,
) -> Result<EditResult> {
    project.check_edit_revision(revision)?;
    let recipe = project
        .data
        .parts
        .get(part)
        .ok_or_else(|| format!("Unknown part: {part}"))?
        .clone();
    if matches!(
        &recipe.geometry,
        Geometry::Imported {
            purchased: Some(_),
            ..
        }
    ) {
        return Err(format!(
            "Part {part} is a purchased part. A change is not permitted."
        ));
    }
    let mesh = project.imported_mesh(part)?;
    let copy = match &operation {
        MeshEdit::Cut { copy_as, .. } | MeshEdit::ToSolid { copy_as, .. } => copy_as.as_deref(),
        _ => None,
    };
    if let Some(id) = copy
        && (!faber_core::valid_id(id)
            || project
                .data
                .parts
                .keys()
                .any(|k| k.eq_ignore_ascii_case(id)))
    {
        return Err(format!("Use a new valid part id for the copy: {id}."));
    }
    if let Some(source) = project.source() {
        return Err(format!(
            "This project is generated from {source}. Edit the source, then call build."
        ));
    }
    let target = copy.unwrap_or(part).to_string();
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut outputs = vec![];
    let mut edits = vec![];
    let mut sources = BTreeMap::new();
    let mut parts = vec![];
    let log = match operation {
        MeshEdit::Repair(options) => {
            let (new, log) = faber_meshtools::try_repair(mesh, &options)?;
            outputs.push((target, new));
            json!(log)
        }
        MeshEdit::Reduce(target_count) => {
            let (new, log) = faber_meshtools::reduce(mesh, target_count)?;
            if log.changed {
                outputs.push((target, new));
            }
            json!(log)
        }
        MeshEdit::Cut {
            origin,
            normal,
            keep,
            cap,
            copy_as,
        } => {
            let place = faber_core::resolved_placements(&project.data)?[part].clone();
            let inverse = faber_core::rotation_matrix(place.rotation).transpose();
            let local_point =
                inverse * (DVec3::from_array(origin) - DVec3::from_array(place.translation));
            let local_normal = inverse * DVec3::from_array(normal);
            let mut sides =
                faber_meshtools::cut(mesh, local_point.into(), local_normal.into(), keep, cap)?;
            let counts: Vec<_> = sides.iter().map(|s| s.triangles.len()).collect();
            let lower = sides.remove(0);
            if keep == Keep::Both {
                outputs.push((part.to_string(), lower));
                if let Some(upper) = sides.pop() {
                    let name = copy_as.unwrap_or_else(|| free_split_id(project, part));
                    outputs.push((name, upper));
                }
            } else {
                outputs.push((target, lower));
            }
            json!({"triangles":counts,"cap":cap,"keep":keep})
        }
        MeshEdit::ToSolid { merge_planar, .. } => {
            let solid = OcctKernel.solid_from_mesh(mesh, merge_planar)?;
            let props = OcctKernel.summary_properties(&solid)?;
            let path = scratch.path().join("solid.step");
            OcctKernel.write_shape(&solid, &path)?;
            let key = project.asset_key(&path)?;
            sources.insert(key.clone(), path);
            let mut new = recipe.clone();
            new.geometry = Geometry::History {
                features: vec![Feature {
                    id: "base".into(),
                    suppressed: false,
                    geometry: Geometry::Imported {
                        asset: key,
                        purchased: None,
                    },
                }],
                output: "base".into(),
            };
            edits.push(Edit::PutPart {
                id: target.clone(),
                part: new,
            });
            parts.push(target);
            json!({"triangles":mesh.triangles.len(),"faces":props.faces,"solids":props.solids,"volume_mm3":props.volume,"merge_planar":merge_planar})
        }
    };
    for (index, (id, output)) in outputs.into_iter().enumerate() {
        let same = output.vertices == mesh.vertices && output.triangles == mesh.triangles;
        if same && id == part {
            continue;
        }
        let mut new = recipe.clone();
        if !same {
            let path = scratch.path().join(format!("mesh-{index}.off"));
            let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            faber_files::write_indexed_mesh(&output, file)?;
            let key = project.asset_key(&path)?;
            sources.insert(key.clone(), path);
            new.geometry = Geometry::<Scalar>::Imported {
                asset: key,
                purchased: None,
            };
        }
        parts.push(id.clone());
        edits.push(Edit::PutPart { id, part: new });
    }
    if !edits.is_empty() {
        project.batch_with_assets(revision, edits, sources)?;
    }
    Ok(EditResult {
        changed: project.data.revision != revision,
        revision: project.data.revision,
        parts,
        log,
    })
}

fn free_split_id(project: &Project, id: &str) -> String {
    for i in 1.. {
        let suffix = if i == 1 {
            "_above".to_string()
        } else {
            format!("_above_{i}")
        };
        let candidate = format!("{}{suffix}", &id[..id.len().min(100 - suffix.len())]);
        if !project
            .data
            .parts
            .keys()
            .any(|k| k.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    unreachable!()
}
