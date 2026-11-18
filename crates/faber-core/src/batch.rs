use crate::body_cache;
use crate::placement::{AlignMode, Axis, Motion, align_edit, move_edit};
use crate::{Project, candidate_local, resolve_part};
use faber_model::{Contact, Edit, Feature, Geometry, ProjectData, Scalar};
use faber_ports::Result;
use schemars::JsonSchema;
use serde::Deserialize;

/// One batch operation: a model edit, a metadata change or a placement.
#[derive(Clone, Debug)]
pub enum Operation {
    SetPartInfo(PartInfoOperation),
    Edit(Box<Edit>),
    /// Moves a part. Its box is read from the state before this operation.
    Move {
        id: String,
        motion: Motion,
        copy_as: Option<String>,
    },
    /// Aligns a part. The boxes are read from the state before this operation.
    Align {
        id: String,
        target: String,
        axes: Vec<Axis>,
        mode: AlignMode,
        gap_mm: f64,
    },
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
    /// Material id. An empty string clears the material.
    pub material: Option<String>,
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
    if info.material.is_none()
        && info.name.is_none()
        && info.colour.is_none()
        && info.group.is_none()
        && info.note.is_none()
    {
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
    if let Some(id) = info.material {
        part.material = (!id.is_empty()).then_some(id);
    }
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
            if let Some(previous) = data.parts.get(&id)
                && matches!(
                    &previous.geometry,
                    Geometry::Imported {
                        purchased: Some(_),
                        ..
                    }
                )
                && previous.geometry != part.geometry
            {
                return Err(format!("Purchased part geometry is fixed: {id}"));
            }
            data.parts.insert(id, part);
        }
        Edit::RemovePart { id } => {
            data.parts
                .remove(&id)
                .ok_or_else(|| format!("Unknown part: {id}"))?;
            data.contacts.retain(|_, c| c.a != id && c.b != id);
            crate::section::detach_part(data, &id)?;
        }
        Edit::SetMotion { study } => data.motion = study,
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
        Edit::SetSuppressed {
            part,
            id,
            suppressed,
            cascade,
        } => {
            let (features, output) = history(data, &part)?;
            let position = index(features, &id)?;
            if features[position].suppressed == suppressed {
                return Ok(());
            }
            if !suppressed {
                let slice = features.as_slice();
                let missing = crate::history::references(&slice[position].geometry)
                    .into_iter()
                    .find(|name| !crate::history::resolves(slice, name));
                if let Some(name) = missing {
                    return Err(format!(
                        "Feature {id} needs {name}, which has no solid. Unsuppress {name} first."
                    ));
                }
                features[position].suppressed = false;
                return Ok(());
            }
            if *output == id
                && crate::history::primary_input(&features[position].geometry).is_none()
            {
                return Err(format!(
                    "The output feature cannot be suppressed. It has no input solid to show: {id}"
                ));
            }
            features[position].suppressed = true;
            // Find each active feature that needs a solid which no longer exists.
            // Refuse it, or with cascade suppress it. A feature with an input still gives a solid.
            loop {
                let slice = features.as_slice();
                let found = slice.iter().find_map(|feature| {
                    if feature.suppressed {
                        return None;
                    }
                    crate::history::references(&feature.geometry)
                        .into_iter()
                        .find(|name| !crate::history::resolves(slice, name))
                        .map(|name| (feature.id.clone(), name.to_owned()))
                });
                let Some((dependant, missing)) = found else {
                    break;
                };
                if !cascade {
                    return Err(format!(
                        "Feature {dependant} needs {missing}, which has no solid. Set cascade to true to suppress {dependant} too."
                    ));
                }
                let index = index(features, &dependant)?;
                features[index].suppressed = true;
            }
            if !crate::history::resolves(features.as_slice(), output) {
                return Err(format!(
                    "Suppressing {id} leaves the output with no solid: {output}"
                ));
            }
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
        self.refuse_generated_operations(&operations)?;
        let candidate = self.candidate(operations)?;
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
        self.refuse_generated_operations(&operations)?;
        self.apply_history(self.candidate(operations)?, None, strict)
    }
}

/// Reads the world box of a part in a candidate state. A part whose recipe did not change
/// reuses its unplaced body. Only a changed or new part is built, without the feature cache.
struct Boxes<'a> {
    project: &'a Project,
}

impl Boxes<'_> {
    fn of(&self, data: &ProjectData, id: &str) -> Result<[[f64; 3]; 2]> {
        let part = data
            .parts
            .get(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        let values = faber_expr::resolve(&data.params)?;
        let resolved = resolve_part(part, &values)?;
        let kernel = self.project.kernel.as_ref();
        let placed = match self
            .project
            .locals
            .get(id)
            .filter(|local| local.geometry == resolved.geometry)
        {
            Some(local) => body_cache::place(local, &resolved.placement, kernel)?,
            None => {
                let local = candidate_local(data, id, kernel, self.project.assets.as_deref())?;
                body_cache::place(&local, &resolved.placement, kernel)?
            }
        };
        Ok(placed.bounds)
    }
}

fn operation(data: &mut ProjectData, boxes: &Boxes<'_>, operation: Operation) -> Result<()> {
    match operation {
        Operation::Edit(edit_operation) => edit(data, *edit_operation),
        Operation::SetPartInfo(PartInfoOperation::SetPartInfo(info)) => part_info(data, info),
        Operation::Move {
            id,
            motion,
            copy_as,
        } => {
            let bounds = boxes.of(data, &id)?;
            let placed = move_edit(data, &id, &motion, bounds, copy_as.as_deref())?;
            edit(data, placed)
        }
        Operation::Align {
            id,
            target,
            axes,
            mode,
            gap_mm,
        } => {
            let current: &ProjectData = data;
            let aligned = align_edit(current, &id, &target, &axes, mode, gap_mm, |part| {
                boxes.of(current, part)
            })?;
            edit(data, aligned)
        }
    }
}

impl Project {
    fn candidate(&self, operations: Vec<Operation>) -> Result<ProjectData> {
        if operations.is_empty() || operations.len() > 2000 {
            return Err("Batch needs between 1 and 2000 operations".into());
        }
        // One operation has no position to name. Several operations name the failing one.
        let several = operations.len() > 1;
        let mut next = self.data.clone();
        let boxes = Boxes { project: self };
        for (position, item) in operations.into_iter().enumerate() {
            operation(&mut next, &boxes, item).map_err(|e| {
                if several {
                    format!("Batch operation {}: {e}", position + 1)
                } else {
                    e
                }
            })?;
        }
        Ok(next)
    }
}
