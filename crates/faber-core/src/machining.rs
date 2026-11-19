use faber_model::{
    ProjectData,
    machining::{Holes, Operation, OperationKind, Selection, Setup, Stock, Tool, ToolKind},
};
use faber_ports::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    SetTool { id: String, definition: Tool },
    SetSetup { id: String, setup: Setup },
    SetOperation { operation: Box<Operation> },
    RemoveOperation { id: String },
}

impl Edit {
    pub(crate) fn apply(self, data: &mut ProjectData) -> Result<()> {
        let m = &mut data.machining;
        match self {
            Self::SetTool { id, definition } => {
                m.tools.insert(id, definition);
            }
            Self::SetSetup { id, setup } => {
                m.setups.insert(id, setup);
            }
            Self::SetOperation { operation } => {
                if let Some(old) = m.operations.iter_mut().find(|op| op.id == operation.id) {
                    *old = *operation;
                } else {
                    m.operations.push(*operation);
                }
            }
            Self::RemoveOperation { id } => {
                let i = m
                    .operations
                    .iter()
                    .position(|op| op.id == id)
                    .ok_or_else(|| format!("Unknown operation: {id}."))?;
                m.operations.remove(i);
            }
        }
        Ok(())
    }
}

fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.
}
fn nonnegative(v: f64) -> bool {
    v.is_finite() && v >= 0.
}
fn selection(s: &Selection) -> bool {
    match s {
        Selection::Z(z) => z.is_finite(),
        Selection::Face(f) => {
            f.expect == 1
                && nonnegative(f.tolerance)
                && f.area.is_none_or(nonnegative)
                && f.cylinder_radius.is_none_or(positive)
                && f.normal
                    .iter()
                    .chain(&f.near_centre)
                    .chain(f.in_box.iter().flatten())
                    .flatten()
                    .all(|v| v.is_finite())
        }
    }
}
fn ids<'a>(items: impl Iterator<Item = &'a String>, kind: &str) -> Result<()> {
    let mut names = BTreeSet::new();
    for id in items {
        if !crate::valid_id(id) || !names.insert(id.to_ascii_lowercase()) {
            return Err(format!("Invalid or duplicate {kind} id: {id}."));
        }
    }
    Ok(())
}
pub fn validate(data: &ProjectData) -> Result<()> {
    let m = &data.machining;
    ids(m.tools.keys(), "tool")?;
    ids(m.setups.keys(), "setup")?;
    ids(m.operations.iter().map(|op| &op.id), "operation")?;
    for (id, t) in &m.tools {
        let laser = t.kind == ToolKind::Laser;
        if !positive(t.diameter)
            || !positive(t.feed_mm_min)
            || !nonnegative(t.plunge_mm_min)
            || !nonnegative(t.flute_length)
            || !nonnegative(t.spindle_rpm)
            || t.number == 0
            || !nonnegative(t.corner_radius)
            || t.corner_radius > t.diameter / 2.
            || (!laser
                && (t.flutes == 0
                    || t.flute_length == 0.
                    || t.plunge_mm_min == 0.
                    || t.spindle_rpm == 0.))
        {
            return Err(format!(
                "Tool {id} has invalid dimensions, number, feed or speed."
            ));
        }
    }
    for (id, s) in &m.setups {
        if !data.parts.contains_key(&s.part) {
            return Err(format!("Setup {id} refers to an unknown part: {}.", s.part));
        }
        let valid = match s.stock {
            Stock::Box { size } => size.into_iter().all(positive),
            Stock::Offset { sides, top, bottom } => {
                [sides, top, bottom].into_iter().all(nonnegative)
            }
        };
        if !valid || glam::DVec3::from(s.up).try_normalize().is_none() {
            return Err(format!("Setup {id} needs valid stock and an up direction."));
        }
    }
    let mut numbers = std::collections::BTreeMap::new();
    for op in &m.operations {
        let id = &op.id;
        let t = m
            .tools
            .get(&op.tool)
            .ok_or_else(|| format!("Operation {id} refers to an unknown tool: {}.", op.tool))?;
        if !m.setups.contains_key(&op.setup) {
            return Err(format!(
                "Operation {id} refers to an unknown setup: {}.",
                op.setup
            ));
        }
        if let Some(previous) = numbers.insert((&op.setup, t.number), t)
            && (previous.kind != t.kind
                || previous.diameter != t.diameter
                || previous.corner_radius != t.corner_radius
                || previous.flute_length != t.flute_length
                || previous.flutes != t.flutes)
        {
            return Err(format!(
                "Setup {} assigns different cutters to tool number {}.",
                op.setup, t.number
            ));
        }
        let h = op.heights;
        if t.kind == ToolKind::Drill && !matches!(op.kind, OperationKind::Drill { .. }) {
            return Err(format!(
                "Operation {id} needs a milling cutter. A drill only makes axial cuts."
            ));
        }
        if ![h.clearance, h.retract, h.top]
            .into_iter()
            .all(f64::is_finite)
            || h.clearance < h.retract
            || h.retract < h.top
            || h.bottom.is_some_and(|z| !z.is_finite() || z > h.top)
            || !op.stepdown.is_none_or(positive)
        {
            return Err(format!(
                "Operation {id} needs ordered heights and a positive stepdown."
            ));
        }
        let valid = match &op.kind {
            OperationKind::Face { stepover } => positive(*stepover),
            OperationKind::Contour {
                z, stock_to_leave, ..
            } => selection(z) && nonnegative(*stock_to_leave),
            OperationKind::Pocket {
                floor,
                stepover,
                stock_to_leave,
            } => selection(floor) && positive(*stepover) && nonnegative(*stock_to_leave),
            OperationKind::Drill { holes, peck } => {
                peck.is_none_or(positive)
                    && match holes {
                        Holes::Auto => true,
                        Holes::List(p) => {
                            !p.is_empty() && p.iter().flatten().all(|x| x.is_finite())
                        }
                    }
            }
            OperationKind::Rough3d {
                stepover,
                stock_to_leave,
            } => positive(*stepover) && nonnegative(*stock_to_leave),
            OperationKind::Parallel3d {
                stepover,
                angle_deg,
            } => positive(*stepover) && angle_deg.is_finite(),
            OperationKind::Cut2d {
                kerf,
                lead_in,
                power,
            } => positive(*kerf) && nonnegative(*lead_in) && positive(*power),
        };
        if !valid || matches!(op.kind, OperationKind::Cut2d { .. }) != (t.kind == ToolKind::Laser) {
            return Err(format!(
                "Check the dimensions and tool type for operation {id}."
            ));
        }
    }
    Ok(())
}
