//! Read and edit a control cage on a part or a feature.
use faber_core::Project;
use faber_form::{Cage, edit::apply_with_origins};
pub use faber_form::{FormOp, Select, primitive_input::Primitive};
use faber_model::{CageData, Edit, Geometry, Part, ProjectData, Scalar, SymmetryPlane};
use faber_ports::Result;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub struct Write {
    pub revision: u64,
    pub strict: bool,
}
impl From<u64> for Write {
    fn from(revision: u64) -> Self {
        Self {
            revision,
            strict: false,
        }
    }
}

fn entry<'a>(
    data: &'a ProjectData,
    part: &str,
    feature: Option<&str>,
) -> Result<(Option<&'a str>, &'a Geometry<Scalar>)> {
    let item = data
        .parts
        .get(part)
        .ok_or_else(|| format!("Unknown part: {part}"))?;
    if let Geometry::History { features, .. } = &item.geometry {
        let mut found = features.iter().filter(|f| {
            feature.map_or(matches!(f.geometry, Geometry::Form { .. }), |id| f.id == id)
        });
        let f = found.next().ok_or("Select a form feature.")?;
        if found.next().is_some() {
            return Err("Select one form feature by name.".into());
        }
        if f.suppressed {
            return Err(format!("Unsuppress form {} before editing it.", f.id));
        }
        if matches!(f.geometry, Geometry::Form { .. }) {
            return Ok((Some(&f.id), &f.geometry));
        }
    } else if feature.is_none() && matches!(item.geometry, Geometry::Form { .. }) {
        return Ok((None, &item.geometry));
    }
    Err(format!("Part {part} does not contain the selected form."))
}
pub fn geometry<'a>(
    data: &'a ProjectData,
    part: &str,
    feature: Option<&str>,
) -> Result<&'a Geometry<Scalar>> {
    Ok(entry(data, part, feature)?.1)
}
pub fn new_form(
    project: &mut Project,
    id: &str,
    cage: CageData,
    levels: Option<u32>,
    as_solid: bool,
    symmetry: Vec<SymmetryPlane>,
    write: impl Into<Write>,
) -> Result<()> {
    let write = write.into();
    project.check_edit_revision(write.revision)?;
    if !faber_core::valid_id(id)
        || project
            .data
            .parts
            .keys()
            .any(|k| k.eq_ignore_ascii_case(id))
    {
        return Err(format!("Use a new valid part id: {id}."));
    }
    let form = Geometry::Form {
        cage: cage.map_numbers(&mut |v| Ok(Scalar::Number(*v)))?,
        levels,
        as_solid,
        symmetry,
    };
    let part = Part {
        name: id.into(),
        geometry: form,
        placement: Default::default(),
        material: None,
        component: None,
        group: String::new(),
        note: String::new(),
        colour: [101, 165, 167],
    };
    project.batch_strict_at_revision(
        write.revision,
        vec![Edit::PutPart {
            id: id.into(),
            part,
        }],
        write.strict,
    )
}
fn change(
    project: &mut Project,
    part: &str,
    feature: Option<&str>,
    write: Write,
    edit: impl FnOnce(&ProjectData, &mut Geometry<Scalar>) -> Result<()>,
) -> Result<()> {
    project.check_edit_revision(write.revision)?;
    let item = project
        .data
        .parts
        .get(part)
        .ok_or_else(|| format!("Unknown part: {part}"))?;
    faber_core::refuse_instance_edit(part, item)?;
    faber_core::refuse_link_edit(part, item)?;
    if matches!(
        item.geometry,
        Geometry::Imported {
            purchased: Some(_),
            ..
        }
    ) {
        return Err(format!(
            "Part {part} is a purchased part. A change is not permitted."
        ));
    }
    let (feature, old) = entry(&project.data, part, feature)?;
    let mut recipe = old.clone();
    edit(&project.data, &mut recipe)?;
    let op = match feature {
        Some(id) => Edit::EditFeature {
            part: part.into(),
            id: id.into(),
            geometry: recipe,
        },
        None => Edit::PutPart {
            id: part.into(),
            part: Part {
                geometry: recipe,
                ..item.clone()
            },
        },
    };
    project.batch_strict_at_revision(write.revision, vec![op], write.strict)
}
pub fn settings(
    project: &mut Project,
    part: &str,
    feature: Option<&str>,
    planes: Vec<SymmetryPlane>,
    level: Option<u32>,
    write: impl Into<Write>,
) -> Result<()> {
    change(project, part, feature, write.into(), |_, recipe| {
        let Geometry::Form {
            symmetry, levels, ..
        } = recipe
        else {
            unreachable!()
        };
        *symmetry = planes;
        *levels = level;
        Ok(())
    })
}
pub fn edit_form(
    project: &mut Project,
    part: &str,
    feature: Option<&str>,
    ops: &[FormOp],
    write: impl Into<Write>,
) -> Result<()> {
    if ops.is_empty() {
        return Err("Supply at least one form edit.".into());
    }
    change(project, part, feature, write.into(), |data, recipe| {
        let Geometry::Form { cage: original, .. } = faber_core::resolve_recipe(data, recipe)?
        else {
            unreachable!()
        };
        let Geometry::Form { cage, symmetry, .. } = recipe else {
            unreachable!()
        };
        let (next, origins) = apply_with_origins(&Cage::from_data(&original)?, ops, symmetry)?;
        let next = next.to_data();
        let mut saved = next.map_numbers(&mut |v| Ok(Scalar::Number(*v)))?;
        for (i, v) in next.vertices.iter().enumerate() {
            if let Some(j) = origins[i] {
                for (axis, &coordinate) in v.iter().enumerate() {
                    if coordinate == original.vertices[j][axis] {
                        saved.vertices[i][axis] = cage.vertices[j][axis].clone();
                    }
                }
            }
        }
        *cage = saved;
        Ok(())
    })
}
pub fn info(project: &Project, part: &str, feature: Option<&str>, detail: bool) -> Result<Value> {
    let (id, recipe) = entry(&project.data, part, feature)?;
    let key = format!("{part}/{}", id.unwrap_or(""));
    let mut value=project.cached("form_info",&key,||{
        let Geometry::Form {cage,levels,as_solid,symmetry}=faber_core::resolve_recipe(&project.data,recipe)? else {unreachable!()};
        let checked=Cage::from_data(&cage)?;
        let mesh=faber_form::subdivide_topology(&checked,levels.unwrap_or(3))?;
        let mut report=faber_meshtools::try_report(&mesh)?;
        let intersects=faber_geometry::bvh::Bvh::new(&mesh)?.intersects_itself();
        if intersects{report.problems.push("The form intersects itself.".into());report.volume_mm3=None;}
        let listed=cage.creases.len().min(16);
        Ok(json!({"part":part,"feature":id,"vertices":cage.vertices.len(),"edges":checked.edges().len(),"faces":cage.faces.len(),"euler":checked.euler(),"symmetry":symmetry,"creases":&cage.creases[..listed],"crease_count":cage.creases.len(),"levels":levels.unwrap_or(3),"as_solid":as_solid,"mesh":report,"intersects_itself":intersects}))
    })?;
    if detail && let Geometry::Form { cage, .. } = recipe {
        value["cage"] = json!(cage);
    }
    Ok(value)
}
