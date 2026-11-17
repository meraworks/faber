//! Broad phase, parallel narrow phase and classification.

use crate::query::{pair_query, penetration_depth};
use crate::report::{Finding, FindingKind, Report, Status};
use crate::scene::{Rules, Scene};
use parry3d_f64::bounding_volume::{Aabb, BoundingVolume};
use parry3d_f64::math::Pose;
use rayon::prelude::*;
use std::time::Instant;

/// Penetration at or below this depth, in millimetres, is contact and not interference.
pub const CONTACT_TOLERANCE_MM: f64 = 1e-6;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Scan the neutral pose only.
    pub static_only: bool,
    /// Worker threads; 0 uses six. Values above six are limited to six.
    pub threads: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            static_only: false,
            threads: 6,
        }
    }
}

fn eligible_pairs(scene: &Scene) -> Vec<(usize, usize)> {
    let n = scene.parts.len();
    let mut pairs = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&scene.parts[i], &scene.parts[j]);
            if a.kind.is_check_volume() && b.kind.is_check_volume() {
                continue;
            }
            if scene
                .rules
                .excluded_pairs
                .contains(&Rules::key(&a.name, &b.name))
            {
                continue;
            }
            pairs.push((i, j));
        }
    }
    pairs
}

pub fn scan(scene: &Scene, options: &ScanOptions) -> Result<Report, String> {
    validate(scene)?;
    let threads = if options.threads == 0 {
        6
    } else {
        options.threads.min(6)
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| format!("Cannot start scan workers: {e}"))?;
    Ok(pool.install(|| scan_inner(scene, options)))
}

fn validate(scene: &Scene) -> Result<(), String> {
    if !scene.rules.proximity_mm.is_finite() || scene.rules.proximity_mm < 0. {
        return Err("Proximity must be finite and non-negative".into());
    }
    let mut names = std::collections::HashSet::new();
    for part in &scene.parts {
        if part.name.trim().is_empty() || !names.insert(part.name.as_str()) {
            return Err("Part names must be non-empty and unique".into());
        }
        if part.mesh.vertices().iter().any(|p| !p.is_finite()) {
            return Err(format!("Part {} has a non-finite vertex", part.name));
        }
    }
    for pose in &scene.poses {
        if pose.transforms.len() != scene.parts.len() {
            return Err("Each pose needs one transform per part".into());
        }
        if pose.label.iter().any(|v| !v.is_finite())
            || pose.transforms.iter().any(|p| !p.is_finite())
        {
            return Err("Pose values must be finite".into());
        }
    }
    for (a, b) in scene
        .rules
        .allowed_contacts
        .iter()
        .chain(&scene.rules.excluded_pairs)
        .chain(scene.rules.allowed_interference.keys())
        .chain(scene.rules.contact_limits_mm.keys())
    {
        if a == b || !names.contains(a.as_str()) || !names.contains(b.as_str()) {
            return Err(format!("Rule needs two distinct known parts: {a}, {b}"));
        }
    }
    if scene
        .rules
        .contact_limits_mm
        .values()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("Contact depth limit must be finite and not negative".into());
    }
    if scene
        .rules
        .allowed_interference
        .values()
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("Allowed interference depth must be finite and non-negative".into());
    }
    Ok(())
}

fn scan_inner(scene: &Scene, options: &ScanOptions) -> Report {
    let started = Instant::now();
    let n = scene.parts.len();
    let proximity = scene.rules.proximity_mm;
    let pairs = eligible_pairs(scene);

    // The neutral pose is prepended unless the scene already lists one.
    let neutral_listed = !options.static_only && scene.neutral_pose_index().is_some();
    let mut poses: Vec<(Vec<f64>, Vec<Pose>)> = if neutral_listed {
        Vec::new()
    } else {
        vec![(vec![0.0; 4], vec![Pose::IDENTITY; n])]
    };
    if !options.static_only {
        for p in &scene.poses {
            poses.push((p.label.clone(), p.transforms.clone()));
        }
    }
    let static_pose = poses
        .iter()
        .position(|(l, _)| l.iter().all(|v| *v == 0.0))
        .unwrap_or(0);

    // Broad phase: loosened AABBs; pairs of immovable parts only in the static pose.
    let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
    let mut rejected = 0usize;
    for (pi, (_, transforms)) in poses.iter().enumerate() {
        let boxes: Vec<Aabb> = scene
            .parts
            .iter()
            .zip(transforms)
            .map(|(p, t)| p.mesh.aabb(t).loosened(proximity))
            .collect();
        for &(i, j) in &pairs {
            if pi != static_pose && !scene.parts[i].movable && !scene.parts[j].movable {
                continue;
            }
            if boxes[i].intersects(&boxes[j]) {
                candidates.push((pi, i, j));
            } else {
                rejected += 1;
            }
        }
    }
    let pair_tests = candidates.len();

    let mut raw: Vec<(usize, Finding)> = candidates
        .par_iter()
        .filter_map(|&(pi, i, j)| {
            let (label, transforms) = &poses[pi];
            let (a, b) = (&scene.parts[i], &scene.parts[j]);
            let r = pair_query(&a.mesh, &transforms[i], &b.mesh, &transforms[j], proximity);
            if r.intersecting {
                let depth = penetration_depth(
                    &r.contact_vertices.0,
                    &b.mesh,
                    &transforms[j],
                    &r.contact_vertices.1,
                    &a.mesh,
                    &transforms[i],
                );
                Some((
                    pi,
                    Finding {
                        pose: label.clone(),
                        a: a.name.clone(),
                        b: b.name.clone(),
                        kind: FindingKind::Interference,
                        depth_mm: depth,
                        surface_crossing: r.surface_crossing,
                        gap_mm: 0.0,
                    },
                ))
            } else if r.gap < proximity {
                Some((
                    pi,
                    Finding {
                        pose: label.clone(),
                        a: a.name.clone(),
                        b: b.name.clone(),
                        kind: FindingKind::Clearance,
                        depth_mm: 0.0,
                        surface_crossing: false,
                        gap_mm: r.gap,
                    },
                ))
            } else {
                None
            }
        })
        .collect();
    raw.sort_by_key(|(pi, _)| *pi);

    let mut violations = Vec::new();
    let mut contacts = Vec::new();
    let mut allowed_contacts = Vec::new();
    let mut allowed_interference = Vec::new();
    // Seated faces come out as intersecting within tessellation error.
    let touch_tolerance = 2.0 * (proximity - 0.005).max(0.0);
    for (_, mut f) in raw {
        let key = Rules::key(&f.a, &f.b);
        let kind_of = |name: &str| {
            scene
                .parts
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.kind)
                .unwrap()
        };
        let check_volume = kind_of(&f.a).is_check_volume() || kind_of(&f.b).is_check_volume();
        let allowed_depth = scene.rules.allowed_interference.get(&key).copied();
        let contact_ok = scene.rules.allowed_contacts.contains(&key);
        // A declared contact allows touch and a press up to its limit.
        // A deeper overlap stays a violation. The caller confirms it exactly.
        let within_contact_limit = scene
            .rules
            .contact_limits_mm
            .get(&key)
            .is_none_or(|limit| f.depth_mm <= *limit);
        // A crossing without a depth sample can hide a real overlap, so it stays refused.
        let unmeasured_crossing = f.surface_crossing && f.depth_mm <= 1e-9;
        let measured = !check_volume && !unmeasured_crossing;
        match f.kind {
            FindingKind::Interference => {
                if measured && allowed_depth.is_some_and(|limit| f.depth_mm <= limit) {
                    f.kind = FindingKind::AllowedInterference;
                    allowed_interference.push(f);
                } else if measured
                    && ((contact_ok && within_contact_limit)
                        || (allowed_depth.is_some() && f.depth_mm <= touch_tolerance))
                {
                    f.kind = FindingKind::AllowedContact;
                    allowed_contacts.push(f);
                } else if !unmeasured_crossing && f.depth_mm <= CONTACT_TOLERANCE_MM {
                    f.kind = FindingKind::Contact;
                    contacts.push(f);
                } else {
                    violations.push(f);
                }
            }
            FindingKind::Clearance => {
                if !check_volume && (contact_ok || allowed_depth.is_some()) {
                    f.kind = FindingKind::AllowedContact;
                    allowed_contacts.push(f);
                } else {
                    violations.push(f);
                }
            }
            _ => unreachable!(),
        }
    }
    let max_depth = allowed_interference
        .iter()
        .map(|f| f.depth_mm)
        .fold(0.0, f64::max);
    Report {
        status: if violations.is_empty() { Status::Pass } else { Status::Fail },
        signature: scene.signature.clone(),
        part_count: n,
        pose_count: poses.len(),
        pair_tests,
        aabb_rejected: rejected,
        violations,
        contacts,
        allowed_contacts,
        allowed_interference,
        max_allowed_interference_depth_mm: max_depth,
        proximity_mm: proximity,
        seconds: started.elapsed().as_secs_f64(),
        method: "faber-clash: paired BVH traversal, triangle intersection and distance, vertex depth estimates; surface crossings without a depth sample are refused".into(),
    }
}
