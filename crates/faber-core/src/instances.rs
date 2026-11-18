//! A part instance shares its source's unplaced body.
use crate::{Motion, Operation, Project, valid_id};
use faber_model::{Geometry, Part, ProjectData, Scalar};
use faber_ports::Result;

pub(crate) fn owner<'a>(data: &'a ProjectData, id: &'a str) -> Result<&'a str> {
    let p = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    let Geometry::Instance { of } = &p.geometry else {
        return Ok(id);
    };
    let source = data
        .parts
        .get(of)
        .ok_or_else(|| format!("Part {id} refers to an unknown instance source: {of}."))?;
    if matches!(source.geometry, Geometry::Instance { .. }) {
        return Err(format!(
            "Part {id} cannot be an instance of {of}: {of} is an instance."
        ));
    }
    Ok(of)
}
pub(crate) fn validate(data: &ProjectData) -> Result<()> {
    for (id, p) in &data.parts {
        if matches!(p.geometry, Geometry::Instance { .. }) {
            owner(data, id)?;
        }
    }
    Ok(())
}
pub(crate) fn check_removal(data: &ProjectData, id: &str) -> Result<()> {
    let copies: Vec<_> = data
        .parts
        .iter()
        .filter(|(_, p)| matches!(&p.geometry,Geometry::Instance{of} if of==id))
        .map(|(id, _)| id.as_str())
        .collect();
    if !copies.is_empty() {
        return Err(format!("Part {id} has instances: {}.", copies.join(", ")));
    }
    Ok(())
}
/// Refuse a geometry change on an occurrence and name its source.
pub fn refuse_edit(id: &str, p: &Part<Scalar>) -> Result<()> {
    if let Geometry::Instance { of } = &p.geometry {
        return Err(format!(
            "Part {id} is an instance of {of}. Change part {of}."
        ));
    }
    Ok(())
}
/// The material of an occurrence. A blank override inherits the source material.
pub fn part_material<'a>(data: &'a ProjectData, id: &str) -> Option<&'a str> {
    let p = data.parts.get(id)?;
    p.material.as_deref().or_else(|| match &p.geometry {
        Geometry::Instance { of } => data.parts.get(of)?.material.as_deref(),
        _ => None,
    })
}

pub fn instance_part(data: &ProjectData, source: &str, copy_as: &str) -> Result<Part<Scalar>> {
    let mut part = data
        .parts
        .get(source)
        .ok_or_else(|| format!("Unknown part: {source}"))?
        .clone();
    if matches!(part.geometry, Geometry::Instance { .. }) {
        return Err(format!(
            "Part {copy_as} cannot be an instance of {source}: {source} is an instance."
        ));
    }
    if !valid_id(copy_as) || data.parts.keys().any(|id| id.eq_ignore_ascii_case(copy_as)) {
        return Err(format!(
            "The new part id is invalid or already in use: {copy_as}."
        ));
    }
    part.geometry = Geometry::Instance {
        of: source.to_owned(),
    };
    part.material = None;
    Ok(part)
}
/// Copy the occurrence and give it the source recipe or the last imported asset.
pub fn independent_part(data: &ProjectData, id: &str) -> Result<Part<Scalar>> {
    let of = owner(data, id)?;
    let source = &data.parts[of];
    crate::placement::refuse_purchased_copy(of, source)?;
    let mut part = data.parts[id].clone();
    if of != id {
        part.geometry = source.geometry.clone();
        part.material = part_material(data, id).map(str::to_owned);
    }
    if let Geometry::Imported { link, .. } = &mut part.geometry {
        *link = None;
    }
    Ok(part)
}
pub(crate) fn detach(data: &mut ProjectData, id: &str) -> Result<()> {
    if owner(data, id)? == id {
        return Err(format!("Part {id} is not an instance."));
    }
    let copy = independent_part(data, id)?;
    data.parts.insert(id.to_owned(), copy);
    Ok(())
}
impl Project {
    pub fn move_linked(&mut self, id: &str, motion: &Motion, copy_as: &str) -> Result<()> {
        self.batch_operations(
            vec![Operation::MoveLinked {
                id: id.into(),
                motion: motion.clone(),
                copy_as: copy_as.into(),
            }],
            false,
        )
    }
    pub fn make_independent(&mut self, id: &str) -> Result<()> {
        self.batch_operations(vec![Operation::MakeIndependent { id: id.into() }], false)
    }
}
