use faber_model::{ProjectData, motion::Pose};
use faber_ports::Result;
use std::collections::BTreeSet;

/// Inherit joint coordinates from the referenced views, then apply local values.
pub fn pose(data: &ProjectData, name: &str) -> Result<Pose> {
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    let mut id = Some(name);
    while let Some(key) = id {
        if !seen.insert(key) {
            return Err(format!("Cycle in saved poses at view {key}."));
        }
        let view = data
            .views
            .get(key)
            .ok_or_else(|| format!("Unknown view: {key}"))?;
        chain.push(&view.joints);
        id = view.pose.as_deref();
    }
    Ok(chain.into_iter().rev().flat_map(|p| p.clone()).collect())
}

pub(crate) fn validate_poses(data: &ProjectData) -> Result<()> {
    for (name, view) in &data.views {
        if view.pose.is_none() && view.joints.is_empty() {
            continue;
        }
        let inputs = pose(data, name).map_err(|e| format!("View {name}: {e}"))?;
        if !inputs.is_empty() {
            data.motion
                .solve_pose(&inputs)
                .map_err(|e| format!("View {name}: {e}"))?;
        }
    }
    Ok(())
}
