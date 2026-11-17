use crate::Project;
use faber_model::{Contact, Edit, Feature, Geometry, ProjectData, Scalar};
use faber_ports::Result;
use schemars::JsonSchema;
use serde::Deserialize;

/// One batch operation: a model edit or a metadata change.
// The server reads each item by its tool field. This type gives the schema.
#[derive(Clone, Debug, JsonSchema)]
#[serde(untagged)]
pub enum Operation {
    SetPartInfo(PartInfoOperation),
    Edit(Box<Edit>),
}

impl From<Edit> for Operation {
    fn from(edit: Edit) -> Self {
        Self::Edit(Box::new(edit))
    }
}

impl Operation {
    pub fn set_part_info(info: PartInfo) -> Self {
        Self::SetPartInfo(PartInfoOperation::SetPartInfo(info))
    }
}

/// Changes only the given fields of a part. The geometry does not change.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PartInfo {
    /// Part id.
    pub id: String,
    /// New name. Must not be blank.
    pub name: Option<String>,
    /// New colour [R,G,B], 0 to 255.
    pub colour: Option<[u8; 3]>,
    /// New group name.
    pub group: Option<String>,
    /// New note.
    pub note: Option<String>,
}

/// Tagged form of a metadata change in a batch.
#[derive(Clone, Debug, JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartInfoOperation {
    SetPartInfo(PartInfo),
}

fn part_info(data: &mut ProjectData, info: PartInfo) -> Result<()> {
    if info.name.is_none() && info.colour.is_none() && info.group.is_none() && info.note.is_none() {
        return Err("Set at least one part field to change".into());
    }
    if info
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err("Part needs a name".into());
    }
    let part = data
        .parts
        .get_mut(&info.id)
        .ok_or_else(|| format!("Unknown part: {}", info.id))?;
    if let Some(name) = info.name {
        part.name = name;
    }
    if let Some(colour) = info.colour {
        part.colour = colour;
    }
    if let Some(group) = info.group {
        part.group = group;
    }
    if let Some(note) = info.note {
        part.note = note;
    }
    Ok(())
}

fn index(features: &[Feature<Scalar>], id: &str) -> Result<usize> {
    features
        .iter()
        .position(|feature| feature.id == id)
        .ok_or_else(|| format!("Unknown feature: {id}"))
}

fn history<'a>(
    data: &'a mut ProjectData,
    id: &str,
) -> Result<(&'a mut Vec<Feature<Scalar>>, &'a mut String)> {
    let part = data
        .parts
        .get_mut(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    let Geometry::History { features, output } = &mut part.geometry else {
        return Err("Part has no feature history".into());
    };
    Ok((features, output))
}

fn edit(data: &mut ProjectData, edit: Edit) -> Result<()> {
    match edit {
        Edit::SetParam { name, expression } => {
            data.params.insert(name, expression);
        }
        Edit::PutPart { id, part } => {
            data.parts.insert(id, part);
        }
        Edit::RemovePart { id } => {
            data.parts
                .remove(&id)
                .ok_or_else(|| format!("Unknown part: {id}"))?;
            data.contacts.retain(|_, c| c.a != id && c.b != id);
            crate::section::detach_part(data, &id)?;
        }
        Edit::SetSection { id, section } => {
            crate::section::check_refs(data, &id, &section)?;
            data.sections.insert(id, section);
        }
        Edit::RemoveSection { id } => {
            data.sections
                .remove(&id)
                .ok_or_else(|| format!("Unknown section: {id}"))?;
        }
        Edit::SetContact {
            a,
            b,
            allowed,
            note,
            max_depth_mm,
        } => {
            if a == b {
                return Err("A contact needs two different parts".into());
            }
            for id in [&a, &b] {
                if !data.parts.contains_key(id) {
                    return Err(format!("Unknown part: {id}"));
                }
            }
            if note.chars().count() > 500 {
                return Err("Contact note has more than 500 characters".into());
            }
            let max_depth_mm = max_depth_mm.unwrap_or(faber_model::DEFAULT_CONTACT_DEPTH_MM);
            if !max_depth_mm.is_finite() || max_depth_mm < 0. {
                return Err("Contact depth limit must be finite and not negative".into());
            }
            let (a, b) = if a < b { (a, b) } else { (b, a) };
            let key = crate::contact_key(&a, &b);
            if allowed {
                data.contacts.insert(
                    key,
                    Contact {
                        a,
                        b,
                        note,
                        max_depth_mm,
                    },
                );
            } else {
                data.contacts.remove(&key);
            }
        }
        Edit::AddFeature {
            part,
            feature,
            before,
            make_output,
        } => {
            let (features, output) = history(data, &part)?;
            let position = before
                .as_deref()
                .map(|id| index(features, id))
                .transpose()?
                .unwrap_or(features.len());
            if make_output {
                *output = feature.id.clone();
            }
            features.insert(position, feature);
        }
        Edit::EditFeature { part, id, geometry } => {
            let (features, _) = history(data, &part)?;
            let position = index(features, &id)?;
            features[position].geometry = geometry;
        }
        Edit::DeleteFeature { part, id } => {
            let (features, _) = history(data, &part)?;
            let position = index(features, &id)?;
            features.remove(position);
        }
        Edit::MoveFeature { part, id, before } => {
            let (features, _) = history(data, &part)?;
            let position = index(features, &id)?;
            if before.as_deref() == Some(&id) {
                return Ok(());
            }
            let feature = features.remove(position);
            let target = before
                .as_deref()
                .map(|id| index(features, id))
                .transpose()?
                .unwrap_or(features.len());
            features.insert(target, feature);
        }
    }
    Ok(())
}

fn to_operations(edits: Vec<Edit>) -> Vec<Operation> {
    edits.into_iter().map(Operation::from).collect()
}

impl Project {
    pub fn preview_batch(
        &mut self,
        operations: Vec<Edit>,
        strict: bool,
    ) -> Result<(ProjectData, Vec<faber_ports::Body>)> {
        self.preview_operations(to_operations(operations), strict)
    }
    pub fn preview_operations(
        &mut self,
        operations: Vec<Operation>,
        strict: bool,
    ) -> Result<(ProjectData, Vec<faber_ports::Body>)> {
        self.sync()?;
        let candidate = candidate(&self.data, operations)?;
        let prepared = self.prepare(candidate, strict)?;
        Ok((prepared.data, prepared.bodies))
    }
    pub fn batch(&mut self, operations: Vec<Edit>) -> Result<()> {
        self.batch_operations(to_operations(operations), false)
    }
    pub fn batch_strict(&mut self, operations: Vec<Edit>) -> Result<()> {
        self.batch_operations(to_operations(operations), true)
    }
    pub fn batch_operations(&mut self, operations: Vec<Operation>, strict: bool) -> Result<()> {
        self.sync()?;
        self.apply_history(candidate(&self.data, operations)?, None, strict)
    }
}

fn operation(data: &mut ProjectData, operation: Operation) -> Result<()> {
    match operation {
        Operation::Edit(edit_operation) => edit(data, *edit_operation),
        Operation::SetPartInfo(PartInfoOperation::SetPartInfo(info)) => part_info(data, info),
    }
}

fn candidate(data: &ProjectData, operations: Vec<Operation>) -> Result<ProjectData> {
    if operations.is_empty() || operations.len() > 2000 {
        return Err("Batch needs between 1 and 2000 operations".into());
    }
    let mut next = data.clone();
    for (position, item) in operations.into_iter().enumerate() {
        operation(&mut next, item).map_err(|e| format!("Batch operation {}: {e}", position + 1))?;
    }
    Ok(next)
}
