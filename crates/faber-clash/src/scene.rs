//! Parts, poses and rules in memory.

use crate::package::{self, Manifest};
use glamx::DMat4;
use parry3d_f64::math::{Pose, Vector};
use parry3d_f64::shape::{TriMesh, TriMeshFlags};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Printed,
    Purchased,
    /// Check volume that is not a physical part (cable path, keep-out zone).
    Envelope,
    /// Exhaust or flame hazard volume.
    Flame,
}

impl PartKind {
    pub fn parse(s: &str) -> PartKind {
        match s {
            "purchased" => PartKind::Purchased,
            "envelope" => PartKind::Envelope,
            "flame" => PartKind::Flame,
            _ => PartKind::Printed,
        }
    }

    /// Check volumes are only tested against physical parts.
    pub fn is_check_volume(self) -> bool {
        matches!(self, PartKind::Envelope | PartKind::Flame)
    }
}

/// A part with its mesh in neutral coordinates. The mesh is built with
/// pseudo-normals so point containment queries work.
pub struct Part {
    pub name: String,
    pub kind: PartKind,
    pub movable: bool,
    pub mesh: TriMesh,
}

pub struct PoseSet {
    pub label: Vec<f64>,
    /// One transform per part, identity for parts that do not move.
    pub transforms: Vec<Pose>,
}

pub struct Rules {
    pub allowed_contacts: HashSet<(String, String)>,
    /// Depth limit of each declared contact, in millimetres. Touch is always allowed.
    pub contact_limits_mm: HashMap<(String, String), f64>,
    pub allowed_interference: HashMap<(String, String), f64>,
    pub excluded_pairs: HashSet<(String, String)>,
    pub proximity_mm: f64,
}

impl Rules {
    pub fn key(a: &str, b: &str) -> (String, String) {
        if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        }
    }
}

pub struct Scene {
    pub signature: String,
    pub parts: Vec<Part>,
    pub poses: Vec<PoseSet>,
    pub rules: Rules,
}

/// Converts a 3x4 row-major rigid matrix into a pose.
pub fn pose_from_matrix(m: &[Vec<f64>]) -> Result<Pose, String> {
    if m.len() != 3 || m.iter().any(|r| r.len() != 4) {
        return Err("transform must be a 3x4 matrix".into());
    }
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Transform values must be finite".into());
    }
    let axes: [Vector; 3] = std::array::from_fn(|i| Vector::new(m[0][i], m[1][i], m[2][i]));
    if axes
        .iter()
        .any(|axis| (axis.length_squared() - 1.).abs() > 1e-6)
        || axes[0].dot(axes[1]).abs() > 1e-6
        || axes[0].dot(axes[2]).abs() > 1e-6
        || axes[1].dot(axes[2]).abs() > 1e-6
        || (axes[0].cross(axes[1]).dot(axes[2]) - 1.).abs() > 1e-6
    {
        return Err("Transform must be a rigid rotation and translation".into());
    }
    let c = [
        m[0][0], m[1][0], m[2][0], 0.0, m[0][1], m[1][1], m[2][1], 0.0, m[0][2], m[1][2], m[2][2],
        0.0, m[0][3], m[1][3], m[2][3], 1.0,
    ];
    Ok(Pose::from_mat4(DMat4::from_cols_array(&c)))
}

impl Scene {
    /// Loads a package directory (`scene.json` + `scene.bin`).
    pub fn load(dir: &Path) -> Result<Scene, String> {
        let (manifest, raw) = package::load(dir)?;
        Scene::from_parts(&manifest, raw)
    }

    pub fn from_parts(manifest: &Manifest, raw: Vec<package::RawMesh>) -> Result<Scene, String> {
        let mut parts = Vec::with_capacity(raw.len());
        for (record, mesh) in manifest.parts.iter().zip(raw) {
            let vertices: Vec<Vector> = mesh
                .vertices
                .iter()
                .map(|v| Vector::new(v[0], v[1], v[2]))
                .collect();
            let tri = TriMesh::with_flags(vertices, mesh.triangles, TriMeshFlags::ORIENTED)
                .map_err(|e| format!("{}: {e:?}", record.name))?;
            parts.push(Part {
                name: record.name.clone(),
                kind: PartKind::parse(&record.kind),
                movable: record.movable,
                mesh: tri,
            });
        }
        let index: HashMap<&str, usize> = parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.as_str(), i))
            .collect();
        let mut poses = Vec::with_capacity(manifest.poses.len());
        for record in &manifest.poses {
            let mut transforms = vec![Pose::IDENTITY; parts.len()];
            for (name, m) in &record.transforms {
                let i = *index
                    .get(name.as_str())
                    .ok_or_else(|| format!("transform for unknown part {name}"))?;
                transforms[i] = pose_from_matrix(m)?;
            }
            poses.push(PoseSet {
                label: record.pose.clone(),
                transforms,
            });
        }
        let key = |p: &Vec<String>| -> Result<(String, String), String> {
            if p.len() != 2 {
                return Err("pair must have two names".into());
            }
            Ok(Rules::key(&p[0], &p[1]))
        };
        let rules = Rules {
            allowed_contacts: manifest
                .allowed_contacts
                .iter()
                .map(key)
                .collect::<Result<_, _>>()?,
            allowed_interference: manifest
                .allowed_interference
                .iter()
                .map(|a| {
                    key(&a.pair).map(|k| {
                        (
                            k,
                            a.max_depth_mm
                                .unwrap_or(manifest.default_max_interference_depth_mm),
                        )
                    })
                })
                .collect::<Result<_, _>>()?,
            contact_limits_mm: Default::default(),
            excluded_pairs: manifest
                .excluded_pairs
                .iter()
                .map(key)
                .collect::<Result<_, _>>()?,
            proximity_mm: manifest.proximity_mm,
        };
        Ok(Scene {
            signature: manifest.signature.clone(),
            parts,
            poses,
            rules,
        })
    }

    /// Index of the pose whose label is all zeros, if there is one.
    pub fn neutral_pose_index(&self) -> Option<usize> {
        self.poses
            .iter()
            .position(|p| p.label.iter().all(|v| *v == 0.0))
    }
}
