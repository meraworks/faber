//! Unplaced bodies. A placement change moves a copy of the body. It does not build the part again.

use crate::placement::rotation_matrix;
use faber_model::{Geometry, Placement};
use faber_ports::{Kernel, Mesh, Result, Shape};
use glam::{DMat3, DVec3};
use std::time::Instant;

/// The unplaced result of one part. Valid while the resolved geometry is equal.
#[derive(Clone)]
pub(crate) struct LocalBody {
    pub geometry: Geometry,
    pub shape: Option<Shape>,
    pub mesh: Mesh,
    pub volume: f64,
    /// Unplaced bounds: exact for a shape, from the mesh otherwise.
    pub bounds: [[f64; 3]; 2],
}

/// The placed mesh, bounds and shape of one part.
pub(crate) struct PlacedParts {
    pub mesh: Mesh,
    pub bounds: [[f64; 3]; 2],
    pub shape: Option<Shape>,
}

/// True when every entry of the rotation is 0 or plus or minus 1, as for quarter turns.
fn axis_aligned(rotation: DMat3) -> bool {
    rotation
        .to_cols_array()
        .iter()
        .all(|v| v.abs() < 1e-9 || (v.abs() - 1.).abs() < 1e-9)
}

/// The bounds of a box after a placement. With an axis-aligned rotation, the moved corners
/// give the exact bounds.
fn moved_bounds(bounds: [[f64; 3]; 2], rotation: DMat3, translation: DVec3) -> [[f64; 3]; 2] {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for corner in 0..8 {
        let point = DVec3::new(
            bounds[corner & 1][0],
            bounds[(corner >> 1) & 1][1],
            bounds[(corner >> 2) & 1][2],
        );
        let moved = rotation * point + translation;
        lo = lo.min(moved);
        hi = hi.max(moved);
    }
    [lo.to_array(), hi.to_array()]
}

/// Times of one part build in milliseconds. Each value is zero when that step did not run.
#[derive(Default)]
pub(crate) struct Timing {
    pub history: f64,
    pub properties: f64,
    pub tessellate: f64,
    pub transform: f64,
    pub bounds: f64,
    pub mesh: f64,
    pub track: f64,
}

/// Places the unplaced body. See `place_timed`.
pub(crate) fn place(
    local: &LocalBody,
    placement: &Placement,
    kernel: &dyn Kernel,
) -> Result<PlacedParts> {
    place_timed(local, placement, kernel, &mut Timing::default())
}

/// Places the unplaced body. A shape turned by a multiple of 90 degrees moves its unplaced
/// bounds. Another turn asks the kernel. A mesh-only part takes its bounds from the placed mesh.
pub(crate) fn place_timed(
    local: &LocalBody,
    placement: &Placement,
    kernel: &dyn Kernel,
    timing: &mut Timing,
) -> Result<PlacedParts> {
    let rotation = rotation_matrix(placement.rotation);
    let translation = DVec3::from(placement.translation);
    let started = Instant::now();
    let shape = match &local.shape {
        Some(source) => Some(kernel.transform_shape(source, placement)?),
        None => None,
    };
    timing.transform = elapsed(started);
    let started = Instant::now();
    let exact_bounds = match &shape {
        Some(_) if axis_aligned(rotation) => {
            Some(moved_bounds(local.bounds, rotation, translation))
        }
        Some(shape) => Some(kernel.properties(shape)?.bounds),
        None => None,
    };
    timing.bounds = elapsed(started);
    let started = Instant::now();
    let mut mesh = local.mesh.clone();
    for p in &mut mesh.vertices {
        *p = (rotation * DVec3::from(*p) + translation).to_array();
    }
    for normal in &mut mesh.normals {
        *normal = (rotation * DVec3::from(*normal))
            .normalize_or_zero()
            .to_array();
    }
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in &mesh.vertices {
        lo = lo.min(DVec3::from(*p));
        hi = hi.max(DVec3::from(*p));
    }
    timing.mesh = elapsed(started);
    Ok(PlacedParts {
        mesh,
        bounds: exact_bounds.unwrap_or([lo.to_array(), hi.to_array()]),
        shape,
    })
}

/// Prints one timing line to stderr when `FABER_TIMING` is `1`.
/// `sections` is the time of the section checks of the same change.
pub(crate) fn report(id: &str, timing: &Timing) {
    if timing_enabled() {
        eprintln!(
            "timing {id} history={:.1} properties={:.1} tessellate={:.1} transform={:.1} bounds={:.1} mesh={:.1} track={:.1}",
            timing.history,
            timing.properties,
            timing.tessellate,
            timing.transform,
            timing.bounds,
            timing.mesh,
            timing.track
        );
    }
}

/// Prints the section time of one change, once, when `FABER_TIMING` is `1`.
pub(crate) fn report_sections(total: f64) {
    if timing_enabled() {
        eprintln!("timing sections_total={total:.1}");
    }
}

fn timing_enabled() -> bool {
    std::env::var("FABER_TIMING").is_ok_and(|value| value == "1")
}

/// Milliseconds since `started`.
pub(crate) fn elapsed(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.
}

/// Stores a mesh as little-endian numbers. Each list starts with its count.
pub(crate) fn encode_mesh(mesh: &Mesh) -> Vec<u8> {
    let mut out = Vec::new();
    push_count(&mut out, mesh.vertices.len());
    for vertex in &mesh.vertices {
        push_all(&mut out, vertex.iter().map(|v| v.to_le_bytes()));
    }
    push_count(&mut out, mesh.triangles.len());
    for triangle in &mesh.triangles {
        push_all(&mut out, triangle.iter().map(|i| i.to_le_bytes()));
    }
    push_count(&mut out, mesh.normals.len());
    for normal in &mesh.normals {
        push_all(&mut out, normal.iter().map(|v| v.to_le_bytes()));
    }
    out
}

/// Reads a mesh from `encode_mesh`. Any malformed byte returns None.
pub(crate) fn decode_mesh(bytes: &[u8]) -> Option<Mesh> {
    let mut reader = Reader(bytes);
    let vertices = (0..reader.count()?)
        .map(|_| Some([reader.f64()?, reader.f64()?, reader.f64()?]))
        .collect::<Option<Vec<_>>>()?;
    let triangles = (0..reader.count()?)
        .map(|_| Some([reader.u32()?, reader.u32()?, reader.u32()?]))
        .collect::<Option<Vec<_>>>()?;
    let normals = (0..reader.count()?)
        .map(|_| Some([reader.f64()?, reader.f64()?, reader.f64()?]))
        .collect::<Option<Vec<_>>>()?;
    if !reader.0.is_empty() {
        return None;
    }
    let vertex_count = vertices.len();
    let indices_in_range = triangles
        .iter()
        .all(|t| t.iter().all(|&i| (i as usize) < vertex_count));
    let normals_match = normals.is_empty() || normals.len() == vertex_count;
    if !indices_in_range || !normals_match {
        return None;
    }
    Some(Mesh {
        vertices,
        triangles,
        normals,
    })
}

fn push_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_le_bytes());
}

fn push_all<const N: usize>(out: &mut Vec<u8>, values: impl Iterator<Item = [u8; N]>) {
    for value in values {
        out.extend_from_slice(&value);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.0.split_first_chunk::<N>()?;
        self.0 = tail;
        Some(*head)
    }
    fn count(&mut self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.take()?)).ok()
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_mesh, encode_mesh};
    use faber_ports::Mesh;

    fn sample() -> Mesh {
        Mesh {
            vertices: vec![[0., 0., 0.], [1.5, -2., 3.25], [4., 5., 6.]],
            triangles: vec![[0, 1, 2]],
            normals: vec![[0., 0., 1.]; 3],
        }
    }

    #[test]
    fn mesh_round_trip_keeps_every_value() {
        let mesh = sample();
        let decoded = decode_mesh(&encode_mesh(&mesh)).unwrap();
        assert_eq!(decoded.vertices, mesh.vertices);
        assert_eq!(decoded.triangles, mesh.triangles);
        assert_eq!(decoded.normals, mesh.normals);
    }

    #[test]
    fn truncated_or_extended_mesh_bytes_are_refused() {
        let bytes = encode_mesh(&sample());
        assert!(decode_mesh(&bytes[..bytes.len() - 1]).is_none());
        let mut extended = bytes.clone();
        extended.push(0);
        assert!(decode_mesh(&extended).is_none());
        assert!(decode_mesh(&[]).is_none());
    }

    #[test]
    fn triangle_index_past_the_vertices_is_refused() {
        let mut mesh = sample();
        mesh.triangles = vec![[0, 1, 3]];
        assert!(decode_mesh(&encode_mesh(&mesh)).is_none());
    }

    #[test]
    fn normal_count_must_be_zero_or_the_vertex_count() {
        let mut mesh = sample();
        mesh.normals = vec![[0., 0., 1.]; 2];
        assert!(decode_mesh(&encode_mesh(&mesh)).is_none());
        mesh.normals.clear();
        assert!(decode_mesh(&encode_mesh(&mesh)).is_some());
    }
}
