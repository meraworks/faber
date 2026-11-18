//! A generated project comes from a design source. The source owns its name,
//! its parameters and its parts. Placement, contacts, motion and sections stay
//! editable. A part keeps its recipe, so a change of its placement alone is
//! allowed. A copy of a part is refused, because it adds a part.

use crate::batch::Operation;
use crate::{Project, section};
use faber_model::{Edit, Geometry, Part, ProjectData, Scalar};
use faber_ports::Result;
use std::collections::BTreeMap;

/// Replaces the name, the parameters and the parts of a project. Each kept
/// part keeps its placement. A removed part also loses its contacts and its
/// section entries. A part in a motion joint cannot be removed.
pub fn regenerate(
    current: &ProjectData,
    name: String,
    params: BTreeMap<String, String>,
    parts: BTreeMap<String, Part<Scalar>>,
) -> Result<ProjectData> {
    for (id, old) in &current.parts {
        let purchased = matches!(
            old.geometry,
            Geometry::Imported {
                purchased: Some(_),
                ..
            }
        );
        let kept = parts
            .get(id)
            .is_some_and(|part| part.geometry == old.geometry);
        if purchased && !kept {
            return Err(format!(
                "Part {id} is a purchased part. The design source cannot change it."
            ));
        }
    }
    let mut next = current.clone();
    next.name = name;
    next.params = params;
    let removed: Vec<String> = current
        .parts
        .keys()
        .filter(|id| !parts.contains_key(*id))
        .cloned()
        .collect();
    for id in &removed {
        if let Some((joint, _)) = current
            .motion
            .joints
            .iter()
            .find(|(_, joint)| joint.parts.contains(id))
        {
            return Err(format!(
                "Part {id} is in motion joint {joint}. Remove the joint first."
            ));
        }
        next.contacts.retain(|_, c| c.a != *id && c.b != *id);
        section::detach_part(&mut next, id)?;
    }
    next.parts = parts
        .into_iter()
        .map(|(id, mut part)| {
            if let Some(old) = current.parts.get(&id) {
                part.placement = old.placement.clone();
            }
            (id, part)
        })
        .collect();
    Ok(next)
}

/// True when `part` is the part `current` with another placement only.
fn placement_only(current: Option<&Part<Scalar>>, part: &Part<Scalar>) -> bool {
    current.is_some_and(|old| {
        old.name == part.name
            && old.material == part.material
            && old.colour == part.colour
            && old.group == part.group
            && old.note == part.note
            && old.geometry == part.geometry
    })
}

impl Project {
    /// Saves the parts that a design source gives. The save is one undo step.
    pub fn save_generated(&mut self, data: ProjectData) -> Result<()> {
        self.sync()?;
        self.apply(data)
    }

    /// Moves the history like `undo` or `redo`, then gives the parts of the
    /// source again. The stored state is exact and is the one undo step. The
    /// regenerated state is not an undo step. Each check runs before the step
    /// is saved, so a source that does not build changes nothing.
    pub fn move_history_with(
        &mut self,
        action: faber_ports::HistoryMove,
        regenerate: &dyn Fn(ProjectData) -> Result<ProjectData>,
    ) -> Result<()> {
        self.sync()?;
        self.refuse_broken_source()?;
        let target = self.store.history_target(action)?;
        let next = regenerate(target.clone())?;
        self.prepare(next.clone(), false)?;
        self.apply_history(target, Some(action), false)?;
        let stepped = ProjectData {
            revision: self.data.revision,
            ..next.clone()
        };
        if stepped != self.data {
            self.save_regenerated(next).map_err(|error| {
                format!("The history step was saved, but the parts were not updated: {error}")
            })?;
        }
        Ok(())
    }

    /// Saves the regenerated state. The undo and redo stacks stay as they are.
    fn save_regenerated(&mut self, next: ProjectData) -> Result<()> {
        let crate::Prepared {
            data,
            bodies,
            histories,
            locals,
        } = self.prepare(next, false)?;
        self.store.save_regenerated(&self.data, &data)?;
        self.data = data;
        self.bodies = bodies;
        self.histories = histories;
        self.locals = locals;
        Ok(())
    }

    /// Restores a checkpoint. `regenerate` gives the state to save.
    pub fn restore_with(
        &mut self,
        name: &str,
        regenerate: &dyn Fn(ProjectData) -> Result<ProjectData>,
    ) -> Result<()> {
        self.sync()?;
        self.refuse_broken_source()?;
        if !crate::valid_id(name) {
            return Err("Invalid checkpoint name".into());
        }
        let target = regenerate(self.store.read_checkpoint(name)?)?;
        self.apply(target)
    }

    /// Sets the file name of the design source. `None` means a project with no source.
    pub fn set_source(&mut self, source: Option<String>) {
        self.source = source;
    }

    /// Sets the error of the last design source build. `None` clears it.
    pub fn set_source_error(&mut self, error: Option<String>) {
        self.source_error = error;
    }

    /// The error of the last design source build, if it failed.
    pub fn source_error(&self) -> Option<&str> {
        self.source_error.as_deref()
    }

    /// The file name of the design source, if the project has one.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Refuses a change while the last design source build failed.
    pub(crate) fn refuse_broken_source(&self) -> Result<()> {
        match &self.source_error {
            Some(error) => Err(format!("The design source has an error: {error}")),
            None => Ok(()),
        }
    }

    /// Refuses plain undo, redo and restore of a generated project. They skip the
    /// recipes of the source. The history functions of the wiring do the regeneration.
    pub(crate) fn refuse_plain_history(&self) -> Result<()> {
        match &self.source {
            Some(_) => {
                Err("This project is generated. Use the history function of the wiring.".into())
            }
            None => Ok(()),
        }
    }

    /// Refuses a change to the parts, the parameters or the features of a generated project.
    pub(crate) fn refuse_generated(&self) -> Result<()> {
        match &self.source {
            Some(source) => Err(format!(
                "This project is generated from {source}. Edit the source, then call build."
            )),
            None => Ok(()),
        }
    }

    /// Refuses a part that the source does not give: a new part or a changed recipe.
    pub(crate) fn refuse_generated_part(&self, id: &str, part: &Part<Scalar>) -> Result<()> {
        if self.source.is_some() && !placement_only(self.data.parts.get(id), part) {
            return self.refuse_generated();
        }
        Ok(())
    }

    /// Refuses the operations that change generated data. Other operations stay allowed.
    pub(crate) fn refuse_generated_operations(&self, operations: &[Operation]) -> Result<()> {
        if operations
            .iter()
            .any(|operation| self.changes_generated(operation))
        {
            self.refuse_generated()
        } else {
            Ok(())
        }
    }

    fn changes_generated(&self, operation: &Operation) -> bool {
        match operation {
            Operation::Edit(edit) => match &**edit {
                Edit::SetStudy { .. }
                | Edit::RemoveStudy { .. }
                | Edit::SetMotion { .. }
                | Edit::SetContact { .. }
                | Edit::SetSection { .. }
                | Edit::RemoveSection { .. } => false,
                Edit::PutPart { id, part } => !placement_only(self.data.parts.get(id), part),
                Edit::SetParam { .. }
                | Edit::RemovePart { .. }
                | Edit::AddFeature { .. }
                | Edit::EditFeature { .. }
                | Edit::DeleteFeature { .. }
                | Edit::MoveFeature { .. }
                | Edit::SetSuppressed { .. } => true,
            },
            Operation::SetPartInfo(_) => true,
            // A copy adds a part. A move or an alignment only changes a placement.
            Operation::Move { copy_as, .. } => copy_as.is_some(),
            Operation::Align { .. } => false,
        }
    }
}
