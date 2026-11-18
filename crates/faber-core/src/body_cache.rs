//! Unplaced bodies. A placement change moves a copy of the body. It does not build the part again.

use crate::placement::rotation_matrix;
use faber_model::{Geometry, Placement};
use faber_ports::{Body, EdgeLines, FaceMesh, Kernel, Mesh, Result, Shape};
use glam::{DMat3, DVec3};
use std::time::Instant;

/// The unplaced result of one part. Valid while the resolved geometry is equal.
#[derive(Clone)]
pub(crate) struct LocalBody {
    pub geometry: Geometry,
    pub shape: Option<Shape>,
    pub mesh: Mesh,
    pub face_of_triangle: Vec<u32>,
    pub edge_lines: EdgeLines,
    pub volume: f64,
    /// Unplaced bounds: exact for a shape, from the mesh otherwise.
    pub bounds: [[f64; 3]; 2],
}

/// The triangles of one face in world coordinates.
pub fn face_triangles(body: &Body, face: u32) -> Vec<[[f64; 3]; 3]> {
    body.mesh
        .triangles
        .iter()
        .zip(&body.face_of_triangle)
        .filter(|(_, index)| **index == face)
        .map(|(triangle, _)| triangle.map(|i| body.mesh.vertices[i as usize]))
        .collect()
}

/// The placed mesh, bounds and shape of one part.
pub(crate) struct PlacedParts {
    pub mesh: Mesh,
    pub edge_lines: EdgeLines,
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
    let mut edge_lines = local.edge_lines.clone();
    for p in &mut edge_lines.points {
        *p = (rotation * DVec3::from(*p) + translation).to_array();
    }
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
        edge_lines,
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
fn write_mesh(out: &mut Vec<u8>, faces: &FaceMesh) {
    let mesh = &faces.mesh;
    push_count(out, mesh.vertices.len());
    for vertex in &mesh.vertices {
        push_all(out, vertex.iter().map(|v| v.to_le_bytes()));
    }
    push_count(out, mesh.triangles.len());
    for triangle in &mesh.triangles {
        push_all(out, triangle.iter().map(|i| i.to_le_bytes()));
    }
    push_count(out, mesh.normals.len());
    for normal in &mesh.normals {
        push_all(out, normal.iter().map(|v| v.to_le_bytes()));
    }
    push_all(out, faces.face_of_triangle.iter().map(|i| i.to_le_bytes()));
}

/// Reads a mesh from `write_mesh`. Any malformed byte returns None.
pub(crate) fn decode_mesh(bytes: &[u8]) -> Option<FaceMesh> {
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
    if reader.0.len() != triangles.len().checked_mul(4)? {
        return None;
    }
    let face_of_triangle = (0..triangles.len())
        .map(|_| reader.u32())
        .collect::<Option<Vec<_>>>()?;
    let vertex_count = vertices.len();
    let indices_in_range = triangles
        .iter()
        .all(|t| t.iter().all(|&i| (i as usize) < vertex_count));
    let normals_match = normals.is_empty() || normals.len() == vertex_count;
    if !indices_in_range || !normals_match || face_of_triangle.contains(&0) {
        return None;
    }
    Some(FaceMesh {
        mesh: Mesh {
            vertices,
            triangles,
            normals,
        },
        face_of_triangle,
    })
}

pub(crate) fn encode_body_mesh(mesh: &FaceMesh, lines: &EdgeLines) -> Vec<u8> {
    let mut out = Vec::new();
    push_count(&mut out, lines.points.len());
    for point in &lines.points {
        push_all(&mut out, point.iter().map(|v| v.to_le_bytes()));
    }
    push_count(&mut out, lines.ranges.len());
    push_all(
        &mut out,
        lines.ranges.iter().map(|range| range.end.to_le_bytes()),
    );
    write_mesh(&mut out, mesh);
    out
}

pub(crate) fn decode_body_mesh(bytes: &[u8]) -> Option<(FaceMesh, EdgeLines)> {
    let mut reader = Reader(bytes);
    let point_count = reader.count()?;
    if point_count > reader.0.len() / 24 {
        return None;
    }
    let points = (0..point_count)
        .map(|_| Some([reader.f64()?, reader.f64()?, reader.f64()?]))
        .collect::<Option<Vec<_>>>()?;
    let count = reader.count()?;
    if count > reader.0.len() / 4 {
        return None;
    }
    let mut start = 0;
    let ranges = (0..count)
        .map(|_| {
            let end = reader.u32()?;
            let range = start..end;
            start = end;
            Some(range)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((decode_mesh(reader.0)?, EdgeLines { points, ranges }))
}

pub(crate) fn valid_edge_lines(lines: &EdgeLines, count: u32) -> bool {
    if lines.ranges.len() != count as usize || !lines.points.iter().flatten().all(|v| v.is_finite())
    {
        return false;
    }
    let mut start = 0;
    for range in &lines.ranges {
        if range.start != start || range.end < start || range.end - start == 1 {
            return false;
        }
        start = range.end;
    }
    start as usize == lines.points.len()
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
    use super::{decode_mesh, write_mesh};

    fn encode_mesh(mesh: &faber_ports::FaceMesh) -> Vec<u8> {
        let mut out = Vec::new();
        write_mesh(&mut out, mesh);
        out
    }
    use faber_ports::{FaceMesh, Mesh};

    fn sample() -> FaceMesh {
        FaceMesh {
            mesh: Mesh {
                vertices: vec![[0., 0., 0.], [1.5, -2., 3.25], [4., 5., 6.]],
                triangles: vec![[0, 1, 2]],
                normals: vec![[0., 0., 1.]; 3],
            },
            face_of_triangle: vec![3],
        }
    }

    #[test]
    fn mesh_round_trip_keeps_every_value() {
        let mesh = sample();
        let decoded = decode_mesh(&encode_mesh(&mesh)).unwrap();
        assert_eq!(decoded.mesh.vertices, mesh.mesh.vertices);
        assert_eq!(decoded.mesh.triangles, mesh.mesh.triangles);
        assert_eq!(decoded.mesh.normals, mesh.mesh.normals);
        assert_eq!(decoded.face_of_triangle, mesh.face_of_triangle);
    }

    #[test]
    fn edge_cache_keeps_empty_ranges_and_refuses_invalid_indices() {
        use super::{decode_body_mesh, encode_body_mesh, valid_edge_lines};
        let mut lines = faber_ports::EdgeLines {
            points: vec![[0., 0., 0.], [4., 3., 2.]],
            ranges: vec![0..0, 0..2],
        };
        let bytes = encode_body_mesh(&sample(), &lines);
        let (_, decoded) = decode_body_mesh(&bytes).unwrap();
        assert_eq!(decoded, lines);
        assert!(valid_edge_lines(&decoded, 2));
        assert!(!valid_edge_lines(&decoded, 3));
        for invalid in [vec![0..1, 1..2], vec![0..0, 1..2], vec![0..0, 0..3]] {
            lines.ranges = invalid;
            assert!(!valid_edge_lines(&lines, 2));
        }
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
    fn each_triangle_needs_a_positive_face_index() {
        let mut mesh = sample();
        for invalid in [vec![], vec![0], vec![1, 2]] {
            mesh.face_of_triangle = invalid;
            assert!(decode_mesh(&encode_mesh(&mesh)).is_none());
        }
    }

    #[test]
    fn triangle_index_past_the_vertices_is_refused() {
        let mut mesh = sample();
        mesh.mesh.triangles = vec![[0, 1, 3]];
        assert!(decode_mesh(&encode_mesh(&mesh)).is_none());
    }

    #[test]
    fn normal_count_must_be_zero_or_the_vertex_count() {
        let mut mesh = sample();
        mesh.mesh.normals = vec![[0., 0., 1.]; 2];
        assert!(decode_mesh(&encode_mesh(&mesh)).is_none());
        mesh.mesh.normals.clear();
        assert!(decode_mesh(&encode_mesh(&mesh)).is_some());
    }
}
