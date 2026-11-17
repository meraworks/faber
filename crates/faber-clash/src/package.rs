//! Scene package format.
//!
//! A package is a directory with two files.
//!
//! `scene.json` is the manifest ([`Manifest`]). `scene.bin` holds the meshes:
//! for each part, in manifest order, `vertex_count * 3` little-endian `f64`
//! followed by `triangle_count * 3` little-endian `u32` indices, starting at
//! the part's `offset`.
//!
//! Coordinates are millimetres in the neutral pose. A pose transform is a 3x4
//! row-major rigid matrix (rotation | translation) from neutral to posed
//! world coordinates.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct PartRecord {
    pub name: String,
    /// `printed`, `purchased`, `envelope` (check volume, e.g. a cable path) or `flame`.
    pub kind: String,
    /// True if the part has transforms in some pose.
    pub movable: bool,
    pub vertex_count: usize,
    pub triangle_count: usize,
    /// Byte offset into `scene.bin`.
    pub offset: usize,
    #[serde(default)]
    pub volume_mm3: f64,
}

/// A pose label (any numbers, e.g. joint angles) plus the transform of each
/// movable part. Parts without an entry stay in the neutral pose.
#[derive(Debug, Clone, Deserialize)]
pub struct PoseRecord {
    pub pose: Vec<f64>,
    pub transforms: HashMap<String, Vec<Vec<f64>>>,
}

/// An intended overlap (press fit, thread engagement) with a depth limit.
#[derive(Debug, Clone, Deserialize)]
pub struct AllowedInterference {
    pub pair: Vec<String>,
    #[serde(default)]
    pub max_depth_mm: Option<f64>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub signature: String,
    pub parts: Vec<PartRecord>,
    #[serde(default)]
    pub poses: Vec<PoseRecord>,
    /// Pairs that can touch or press. No depth limit applies to them.
    #[serde(default)]
    pub allowed_contacts: Vec<Vec<String>>,
    /// Pairs that may overlap up to a depth limit.
    #[serde(default)]
    pub allowed_interference: Vec<AllowedInterference>,
    /// Pairs that are never tested.
    #[serde(default)]
    pub excluded_pairs: Vec<Vec<String>>,
    /// Parts closer than this count as touching. About twice the
    /// tessellation deviation is a good value.
    pub proximity_mm: f64,
    /// Depth limit for allowed interferences without their own limit.
    #[serde(default = "default_depth")]
    pub default_max_interference_depth_mm: f64,
    #[serde(default)]
    pub tessellation_tolerance_mm: f64,
}

fn one() -> u32 {
    1
}

fn default_depth() -> f64 {
    0.6
}

pub struct RawMesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Loads the manifest and decodes every mesh.
pub fn load(dir: &Path) -> Result<(Manifest, Vec<RawMesh>), String> {
    let text =
        std::fs::read_to_string(dir.join("scene.json")).map_err(|e| format!("scene.json: {e}"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("scene.json: {e}"))?;
    let bin = std::fs::read(dir.join("scene.bin")).map_err(|e| format!("scene.bin: {e}"))?;
    let mut meshes = Vec::with_capacity(manifest.parts.len());
    for part in &manifest.parts {
        meshes.push(decode(&bin, part)?);
    }
    Ok((manifest, meshes))
}

/// Returns `len` bytes at `at`. Refuses a range that leaves the buffer.
fn bytes_at<'a>(bin: &'a [u8], at: usize, len: usize, name: &str) -> Result<&'a [u8], String> {
    at.checked_add(len)
        .and_then(|end| bin.get(at..end))
        .ok_or_else(|| format!("{name}: scene.bin too short for data at byte {at}"))
}

fn f64_at(bin: &[u8], at: usize, name: &str) -> Result<f64, String> {
    let raw: [u8; 8] = bytes_at(bin, at, 8, name)?
        .try_into()
        .map_err(|_| format!("{name}: bad number at byte {at}"))?;
    Ok(f64::from_le_bytes(raw))
}

fn u32_at(bin: &[u8], at: usize, name: &str) -> Result<u32, String> {
    let raw: [u8; 4] = bytes_at(bin, at, 4, name)?
        .try_into()
        .map_err(|_| format!("{name}: bad index at byte {at}"))?;
    Ok(u32::from_le_bytes(raw))
}

fn decode(bin: &[u8], part: &PartRecord) -> Result<RawMesh, String> {
    let overflow = || format!("{}: part size overflows", part.name);
    let vertex_bytes = part.vertex_count.checked_mul(24).ok_or_else(overflow)?;
    let triangle_bytes = part.triangle_count.checked_mul(12).ok_or_else(overflow)?;
    let end = part
        .offset
        .checked_add(vertex_bytes)
        .and_then(|n| n.checked_add(triangle_bytes))
        .ok_or_else(overflow)?;
    if end > bin.len() {
        return Err(format!(
            "{}: scene.bin too short ({} < {end})",
            part.name,
            bin.len()
        ));
    }
    let mut off = part.offset;
    let mut vertices = Vec::with_capacity(part.vertex_count);
    for _ in 0..part.vertex_count {
        let name = part.name.as_str();
        vertices.push([
            f64_at(bin, off, name)?,
            f64_at(bin, off + 8, name)?,
            f64_at(bin, off + 16, name)?,
        ]);
        off += 24;
    }
    let mut triangles = Vec::with_capacity(part.triangle_count);
    for _ in 0..part.triangle_count {
        let t = [
            u32_at(bin, off, &part.name)?,
            u32_at(bin, off + 4, &part.name)?,
            u32_at(bin, off + 8, &part.name)?,
        ];
        if t.iter().any(|&i| i as usize >= part.vertex_count) {
            return Err(format!("{}: triangle index out of range", part.name));
        }
        triangles.push(t);
        off += 12;
    }
    Ok(RawMesh {
        vertices,
        triangles,
    })
}
