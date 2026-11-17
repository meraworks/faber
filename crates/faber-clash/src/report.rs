//! Findings and the JSON report.

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// Meshes overlap deeper than the contact tolerance and the pair is not allowed.
    Interference,
    /// Meshes touch within the contact tolerance and the pair is not allowed. Not a violation.
    Contact,
    /// Separate meshes closer than proximity and the pair is not allowed.
    Clearance,
    /// Pair that touches or overlaps and is allowed.
    AllowedContact,
    /// Overlapping pair that is allowed and within its depth limit.
    AllowedInterference,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub pose: Vec<f64>,
    pub a: String,
    pub b: String,
    pub kind: FindingKind,
    pub depth_mm: f64,
    /// Non-coplanar surface crossings were found.
    pub surface_crossing: bool,
    pub gap_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Pass,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: Status,
    pub signature: String,
    pub part_count: usize,
    pub pose_count: usize,
    pub pair_tests: usize,
    pub aabb_rejected: usize,
    /// Interference and clearance findings. Each one fails the report.
    pub violations: Vec<Finding>,
    /// Pairs that touch and are not declared. They do not fail the report.
    pub contacts: Vec<Finding>,
    pub allowed_contacts: Vec<Finding>,
    pub allowed_interference: Vec<Finding>,
    pub max_allowed_interference_depth_mm: f64,
    pub proximity_mm: f64,
    pub seconds: f64,
    pub method: String,
}

impl Report {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
