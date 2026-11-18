//! Check interference over one independent joint range.

use crate::{OcctKernel, interference};
use faber_model::motion::{MotionStudy, PoseValue};
use faber_model::{Contact, Placement};
use faber_ports::{Body, Kernel, Result};
use glam::{DQuat, DVec3, EulerRot};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepStatus {
    Clear,
    Collision,
    Uncertain,
}

#[derive(Debug, Serialize)]
pub struct SweepReport {
    pub status: SweepStatus,
    pub samples: usize,
    pub interval: [f64; 2],
    pub parts: Option<[String; 2]>,
}

type Bounds = [[f64; 3]; 2];

fn transform(placement: &Placement) -> (DQuat, DVec3) {
    let [x, y, z] = placement.rotation.map(f64::to_radians);
    (
        DQuat::from_euler(EulerRot::ZYX, z, y, x),
        placement.translation.into(),
    )
}

fn posed_bounds(bounds: Bounds, placement: Option<&Placement>) -> Bounds {
    let Some(placement) = placement else {
        return bounds;
    };
    let (rotation, translation) = transform(placement);
    let [low, high] = bounds;
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = -min;
    for x in [low[0], high[0]] {
        for y in [low[1], high[1]] {
            for z in [low[2], high[2]] {
                let corner = rotation * DVec3::new(x, y, z) + translation;
                min = min.min(corner);
                max = max.max(corner);
            }
        }
    }
    [min.into(), max.into()]
}

fn pose_body(body: &Body, placement: Option<&Placement>, bounds: Bounds) -> Result<Body> {
    let mut body = body.clone();
    let Some(placement) = placement else {
        return Ok(body);
    };
    let (rotation, translation) = transform(placement);
    body.bounds = bounds;
    for vertex in &mut body.mesh.vertices {
        *vertex = (rotation * DVec3::from(*vertex) + translation).into();
    }
    body.shape = body
        .shape
        .as_ref()
        .map(|shape| OcctKernel.transform_shape(shape, placement))
        .transpose()?;
    Ok(body)
}

// Joint dependencies have passed the cycle check in the pose solver.
fn driven<'a>(study: &'a MotionStudy, mut id: &'a str, driver: &str) -> bool {
    loop {
        if id == driver {
            return true;
        }
        if let Some(gear) = study.gears.get(id) {
            id = &gear.driver;
        } else if let Some(link) = study.linkages.values().find(|link| link.follower == id) {
            id = &link.driver;
        } else {
            return false;
        }
    }
}

// Bodies with the same active frame retain their relative position.
fn group<'a>(body: &Body, study: &'a MotionStudy, driver: &str) -> Option<(bool, &'a str)> {
    let mut owner = study
        .joints
        .iter()
        .find(|(_, joint)| joint.parts.contains(&body.id));
    if let Some((id, link)) = study
        .linkages
        .iter()
        .find(|(_, link)| link.rod_parts.contains(&body.id))
    {
        if driven(study, &link.driver, driver) {
            return Some((true, id));
        }
        owner = study.joints[&link.driver]
            .parent
            .as_ref()
            .and_then(|id| study.joints.get_key_value(id));
    }
    while let Some((id, joint)) = owner {
        if driven(study, id, driver) {
            return Some((false, id));
        }
        owner = joint
            .parent
            .as_ref()
            .and_then(|id| study.joints.get_key_value(id));
    }
    None
}

// Bound the distance a point travels per unit of driver travel.
fn speed(
    body: &Body,
    study: &MotionStudy,
    driver: &str,
    coordinate: usize,
    range: [f64; 2],
) -> f64 {
    if study
        .linkages
        .values()
        .any(|link| link.rod_parts.contains(&body.id))
    {
        return f64::INFINITY;
    }
    let mut owner = study
        .joints
        .iter()
        .find(|(_, j)| j.parts.contains(&body.id));

    let [low, high] = body.bounds.map(DVec3::from);
    let mut centre = low * 0.5 + high * 0.5;
    let mut radius = (high - centre).abs().max((low - centre).abs()).length();
    let mut speed = 0.;
    while let Some((id, joint)) = owner {
        let origin = DVec3::from(joint.origin);
        radius += origin.distance(centre);
        centre = origin;
        let mut source = id;
        let mut ratio = 1.;
        while let Some(gear) = study.gears.get(source) {
            ratio *= gear.ratio.abs();
            source = &gear.driver;
        }
        if study.linkages.values().any(|link| &link.follower == source) {
            return f64::INFINITY;
        }
        if source == driver {
            if joint
                .kind
                .angular(if id == driver { coordinate } else { 0 })
            {
                speed += radius * ratio.to_radians();
            } else {
                speed += ratio;
                radius += range[0].abs().max(range[1].abs()) * ratio;
            }
        }
        owner = joint
            .parent
            .as_ref()
            .and_then(|id| study.joints.get_key_value(id));
    }
    speed
}

fn box_gap(a: Bounds, b: Bounds) -> f64 {
    let [al, ah] = a.map(DVec3::from);
    let [bl, bh] = b.map(DVec3::from);
    (bl - ah).max(al - bh).max(DVec3::ZERO).length()
}

/// Other independent joints stay at zero. A clearance violation also stops the sweep.
pub fn sweep(
    bodies: &[Body],
    study: &MotionStudy,
    contacts: &BTreeMap<String, Contact>,
    joint: &str,
    range: [f64; 2],
    proximity_mm: f64,
) -> Result<SweepReport> {
    sweep_coordinate(bodies, study, contacts, joint, 0, range, proximity_mm)
}
pub fn sweep_coordinate(
    bodies: &[Body],
    study: &MotionStudy,
    contacts: &BTreeMap<String, Contact>,
    joint: &str,
    coordinate: usize,
    range: [f64; 2],
    proximity_mm: f64,
) -> Result<SweepReport> {
    if !range.iter().all(|x| x.is_finite()) || !(range[1] - range[0]).is_finite() {
        return Err("Use a finite motion range".into());
    }
    if !proximity_mm.is_finite() || proximity_mm < 0. {
        return Err("Use a finite, non-negative clearance".into());
    }
    let count = study
        .joints
        .get(joint)
        .ok_or_else(|| format!("Unknown joint {joint}"))?
        .kind
        .count();
    if coordinate >= count {
        return Err(format!("Joint {joint} has {count} motion coordinates."));
    }
    let solve = |value| {
        let mut coords = vec![0.; count];
        coords[coordinate] = value;
        study.solve_pose(&BTreeMap::from([(
            joint.to_owned(),
            PoseValue::from_values(coords),
        )]))
    };
    let initial = solve(range[0])?;
    if let Some(id) = initial
        .keys()
        .find(|id| !bodies.iter().any(|b| &b.id == *id))
    {
        return Err(format!("Unknown moving part {id}"));
    }
    let groups: Vec<_> = bodies
        .iter()
        .map(|body| group(body, study, joint))
        .collect();
    let pairs: Vec<_> = (0..bodies.len())
        .flat_map(|i| (i + 1..bodies.len()).map(move |j| (i, j)))
        .filter(|&(i, j)| groups[i] != groups[j])
        .collect();
    let speeds: Vec<_> = bodies
        .iter()
        .zip(groups)
        .map(|(body, group)| {
            if group.is_some() {
                speed(body, study, joint, coordinate, range)
            } else {
                0.
            }
        })
        .collect();
    let moving_linkage = study
        .linkages
        .values()
        .any(|link| driven(study, &link.driver, joint));
    let mut report = SweepReport {
        status: SweepStatus::Clear,
        samples: 0,
        interval: range,
        parts: None,
    };
    let mut intervals = VecDeque::from([[range[0], range[0]], [range[1], range[1]], range]);
    while let Some(interval) = intervals.pop_front() {
        let [a, b] = interval;
        let mid = a + (b - a) * 0.5;
        let placements = solve(mid)?;
        let bounds: Vec<_> = bodies
            .iter()
            .map(|body| posed_bounds(body.bounds, placements.get(&body.id)))
            .collect();
        report.samples += 1;
        let mut unresolved = None;
        for &(i, j) in &pairs {
            let gap = box_gap(bounds[i], bounds[j]);
            if gap <= proximity_mm + 1e-8 {
                // Build meshes only for pairs whose boxes are close at this pose.
                let pair =
                    [i, j].map(|k| pose_body(&bodies[k], placements.get(&bodies[k].id), bounds[k]));
                let [first, second] = pair;
                let check = interference(&[first?, second?], proximity_mm, contacts)?;
                if let Some(f) = check.violations.first() {
                    report.status = SweepStatus::Collision;
                    report.parts = Some([f.a.clone(), f.b.clone()]);
                    report.interval = [mid, mid];
                    return Ok(report);
                }
            }
            let travel = (speeds[i] + speeds[j]) * (mid - a).abs().max((b - mid).abs());
            if unresolved.is_none() && (!travel.is_finite() || gap <= travel + proximity_mm + 1e-8)
            {
                unresolved = Some([bodies[i].id.clone(), bodies[j].id.clone()]);
            }
        }
        if a == b {
            continue;
        }
        // Linkage closure between samples needs a separate interval proof.
        if unresolved.is_some() || moving_linkage {
            if report.samples + intervals.len() >= 65 || mid == a || mid == b {
                if report.status == SweepStatus::Clear {
                    report.status = SweepStatus::Uncertain;
                    report.interval = interval;
                    report.parts = unresolved;
                }
            } else {
                intervals.extend([[a, mid], [mid, b]]);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn checks_between_clear_endpoints() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = crate::create(directory.path(), "Slide".into()).unwrap();
        for (id, x) in [
            ("slide", 0.),
            ("slide_mount", 0.),
            ("stop", 5.),
            ("base_a", -10.),
            ("base_b", -10.),
        ] {
            project
                .put_part(
                    id.into(),
                    serde_json::from_value::<faber_model::Part<f64>>(json!({
                        "name":id,"colour":[0,0,0],"geometry":{"op":"box","size":[1,1,1]},
                        "placement":{"translation":[x,0,0]}
                    }))
                    .unwrap(),
                )
                .unwrap();
        }
        let study = serde_json::from_value(json!({"joints":{"travel":{
            "kind":"slider","parts":["slide","slide_mount"],"axis":[1,0,0],"origin":[0,0,0]
        }}}))
        .unwrap();
        let check = |range| {
            sweep(
                &project.bodies,
                &study,
                &project.data.contacts,
                "travel",
                range,
                0.,
            )
            .unwrap()
        };
        let hit = check([0., 16.]);
        assert_eq!(hit.status, SweepStatus::Collision);
        assert!(hit.interval[0] > 0. && hit.interval[1] < 16.);
        assert_eq!(check([0., 2.]).status, SweepStatus::Clear);
    }
}
