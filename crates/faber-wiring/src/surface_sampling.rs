//! Sample triangle interiors for directional surface measurements.

use faber_ports::{FaceMesh, Result};
use glam::DVec3;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const MAX_ADDED_TRIANGLES: usize = 65_536;
const MAX_TRIANGLES: usize = 500_000;

/// Return the sample mesh and its largest edge in millimetres.
pub fn refine(source: &FaceMesh, chord_mm: f64) -> Result<(FaceMesh, f64)> {
    refine_with_limit(source, chord_mm, MAX_ADDED_TRIANGLES)
}

fn refine_with_limit(source: &FaceMesh, chord_mm: f64, allowed: usize) -> Result<(FaceMesh, f64)> {
    let input = &source.mesh;
    if !chord_mm.is_finite() || chord_mm <= 0. {
        return Err("The sample spacing must be finite and positive.".into());
    }
    if input.triangles.is_empty() || input.triangles.len() > MAX_TRIANGLES {
        return Err(format!(
            "Give 1 to {MAX_TRIANGLES} triangles for surface sampling."
        ));
    }
    if source.face_of_triangle.len() != input.triangles.len()
        || (!input.normals.is_empty() && input.normals.len() != input.vertices.len())
    {
        return Err("The mesh face or normal count is inconsistent.".into());
    }
    let mut low = DVec3::splat(f64::INFINITY);
    let mut high = DVec3::splat(f64::NEG_INFINITY);
    for &index in input.triangles.iter().flatten() {
        let point = DVec3::from(
            *input
                .vertices
                .get(index as usize)
                .ok_or("Invalid surface triangle.")?,
        );
        if !point.is_finite() {
            return Err("Surface coordinates must be finite.".into());
        }
        low = low.min(point);
        high = high.max(point);
    }
    let target = chord_mm.max((high - low).max_element() / 48.);
    let mut result = source.clone();
    let mut edges = EdgeQueue::new(target);
    for index in 0..input.triangles.len() {
        edges.add(&result, index)?;
    }
    let budget = allowed.min(MAX_TRIANGLES - input.triangles.len());
    for _ in 0..budget {
        let Some((_, Reverse(index), side)) = edges.pending.pop() else {
            break;
        };
        let triangle = result.mesh.triangles[index];
        let [a, b, c] = [
            triangle[side],
            triangle[(side + 1) % 3],
            triangle[(side + 2) % 3],
        ];
        let vertices = &mut result.mesh.vertices;
        let mid = u32::try_from(vertices.len()).map_err(|_| "Too many surface vertices.")?;
        vertices.push(std::array::from_fn(|i| {
            vertices[a as usize][i] * 0.5 + vertices[b as usize][i] * 0.5
        }));
        let normals = &mut result.mesh.normals;
        if !normals.is_empty() {
            let n = DVec3::from(normals[a as usize]) + DVec3::from(normals[b as usize]);
            normals.push(n.normalize_or_zero().to_array());
        }
        result.mesh.triangles[index] = [a, mid, c];
        let next = result.mesh.triangles.len();
        result.mesh.triangles.push([mid, b, c]);
        result.face_of_triangle.push(result.face_of_triangle[index]);
        edges.add(&result, index)?;
        edges.add(&result, next)?;
    }
    let maximum = edges
        .pending
        .peek()
        .map_or(edges.settled, |e| f64::from_bits(e.0));
    Ok((result, maximum.sqrt()))
}

struct EdgeQueue {
    target_squared: f64,
    settled: f64,
    // The bit order of a finite non-negative length gives its numeric order.
    pending: BinaryHeap<(u64, Reverse<usize>, usize)>,
}

impl EdgeQueue {
    fn new(spacing: f64) -> Self {
        Self {
            target_squared: spacing * spacing,
            settled: 0.,
            pending: BinaryHeap::new(),
        }
    }

    fn add(&mut self, mesh: &FaceMesh, index: usize) -> Result<()> {
        let p = mesh.mesh.triangles[index].map(|i| DVec3::from(mesh.mesh.vertices[i as usize]));
        let mut longest = 0.;
        let mut side = 0;
        for i in 0..3 {
            let length = p[i].distance_squared(p[(i + 1) % 3]);
            if !length.is_finite() {
                return Err("A triangle exceeds the surface analysis coordinate limit.".into());
            }
            if length > longest {
                longest = length;
                side = i;
            }
        }
        if longest > self.target_squared {
            self.pending.push((longest.to_bits(), Reverse(index), side));
        } else {
            self.settled = self.settled.max(longest);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faber_core::surface::checked_access_report;
    use faber_geometry::bvh::Bvh;
    use faber_ports::Mesh;

    fn plate(low: f64, high: f64, z: f64) -> FaceMesh {
        FaceMesh {
            mesh: Mesh {
                vertices: vec![
                    [low, low, z],
                    [high, low, z],
                    [high, high, z],
                    [low, high, z],
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                normals: vec![[0., 0., 1.]; 4],
            },
            face_of_triangle: vec![17, 31],
        }
    }

    #[test]
    fn access_measures_a_small_shadow_inside_a_large_face() {
        let floor = plate(0., 20., 0.);
        let roof = Bvh::new(&plate(8.5, 11.5, 1.).mesh).unwrap();
        let body = Bvh::new(&floor.mesh).unwrap();
        let measure =
            |m: &FaceMesh| checked_access_report(m, &body, &[[0., 0., 1.]], Some(&roof)).unwrap();
        assert_eq!(measure(&floor).hidden_mm2, 0.);
        let (fine, spacing) = refine(&floor, 0.1).unwrap();
        let r = measure(&fine);
        assert!(
            (r.hidden_mm2 - 9.).abs() < 0.8,
            "Shadow: {} mm²",
            r.hidden_mm2
        );
        assert!((r.reachable_mm2 + r.hidden_mm2 - 400.).abs() < 1e-7);
        assert_eq!(r.hidden_faces, vec![17, 31]);
        assert!(spacing <= 20. / 48.);
        assert_eq!(fine.mesh.normals.len(), fine.mesh.vertices.len());
        assert!(fine.mesh.normals.iter().all(|n| *n == [0., 0., 1.]));
        assert_eq!(fine.mesh.triangles.len(), fine.face_of_triangle.len());
    }

    #[test]
    fn sufficient_spacing_preserves_the_original_mesh() {
        let floor = plate(0., 0.1, 1.);
        let (same, spacing) = refine(&floor, 0.2).unwrap();
        assert_eq!(same.mesh.vertices, floor.mesh.vertices);
        assert_eq!(same.mesh.triangles, floor.mesh.triangles);
        assert_eq!(same.face_of_triangle, floor.face_of_triangle);
        assert!((spacing - 0.1_f64.hypot(0.1)).abs() < 1e-10);
    }

    #[test]
    fn sample_budget_reports_the_actual_spacing() {
        let floor = plate(0., 20., 0.);
        let (fine, spacing) = refine_with_limit(&floor, 0.1, 2).unwrap();
        assert_eq!(fine.mesh.triangles.len(), 4);
        assert_eq!(
            fine.face_of_triangle.iter().filter(|&&id| id == 17).count(),
            2
        );
        assert!((spacing - 20.).abs() < 1e-10);
    }
}
