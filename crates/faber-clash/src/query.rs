//! Pair queries on posed meshes.
//!
//! The two BVHs are traversed together. The second mesh's node boxes are
//! taken into the first mesh's local frame, so parts that move together keep
//! tight boxes (transforming both into world space inflates every rotated
//! box). Only triangle pairs whose boxes are within `proximity` are tested.

use parry3d_f64::bounding_volume::BoundingVolume;
use parry3d_f64::math::{Pose, Vector};
use parry3d_f64::query::{PointQuery, distance, intersection_test};
use parry3d_f64::shape::{TriMesh, Triangle};

pub struct PairResult {
    pub intersecting: bool,
    pub surface_crossing: bool,
    /// Minimum separation when not intersecting; `INFINITY` if nothing is
    /// within proximity.
    pub gap: f64,
    /// World-space vertices of the intersecting triangles of each mesh.
    pub contact_vertices: (Vec<Vector>, Vec<Vector>),
    pub triangle_pairs: usize,
}

pub fn pair_query(
    a: &TriMesh,
    pose_a: &Pose,
    b: &TriMesh,
    pose_b: &Pose,
    proximity: f64,
) -> PairResult {
    let b_in_a = pose_a.inv_mul(pose_b);
    let mut result = PairResult {
        intersecting: false,
        surface_crossing: false,
        gap: f64::INFINITY,
        contact_vertices: (Vec::new(), Vec::new()),
        triangle_pairs: 0,
    };
    let pairs = a.bvh().leaf_pairs(b.bvh(), |n1, n2| {
        n1.aabb()
            .loosened(proximity)
            .intersects(&n2.aabb().transform_by(&b_in_a))
    });
    for (i, j) in pairs {
        result.triangle_pairs += 1;
        let (t1, t2) = (a.triangle(i), b.triangle(j).transformed(&b_in_a));
        if intersection_test(&Pose::IDENTITY, &t1, &Pose::IDENTITY, &t2)
            .map(|r| r.intersecting)
            .unwrap_or(false)
        {
            result.intersecting = true;
            result.surface_crossing |= crosses(&t1, &t2);
            result
                .contact_vertices
                .0
                .extend([t1.a, t1.b, t1.c].map(|p| pose_a.transform_point(p)));
            result
                .contact_vertices
                .1
                .extend([t2.a, t2.b, t2.c].map(|p| pose_a.transform_point(p)));
            continue;
        }
        if !result.intersecting
            && let Ok(d) = distance(&Pose::IDENTITY, &t1, &Pose::IDENTITY, &t2)
        {
            result.gap = result.gap.min(d.distance);
        }
    }
    {
        // Surface tests miss a closed part inside another part. They also miss
        // aligned faces. Containment gives the depth samples in both cases.
        for (source, source_pose, target, target_pose, contacts) in [
            (a, pose_a, b, pose_b, &mut result.contact_vertices.0),
            (b, pose_b, a, pose_a, &mut result.contact_vertices.1),
        ] {
            if target.pseudo_normals_if_oriented().is_none() {
                continue;
            }
            // Face centres too. Aligned faces can leave every vertex on the other surface.
            let centres = (0..source.num_triangles() as u32).map(|i| {
                let t = source.triangle(i);
                (t.a + t.b + t.c) / 3.
            });
            for point in source.vertices().iter().copied().chain(centres) {
                let world = source_pose.transform_point(point);
                let projection = target.project_point(target_pose, world, false);
                if projection.is_inside && (projection.point - world).length() > 1e-9 {
                    contacts.push(world);
                    result.intersecting = true;
                }
            }
        }
    }
    if result.intersecting {
        result.gap = 0.0;
    }
    result
}

fn crosses(a: &Triangle, b: &Triangle) -> bool {
    const EPS: f64 = 1e-8;
    let Some(na) = (a.b - a.a).cross(a.c - a.a).try_normalize() else {
        return false;
    };
    let Some(nb) = (b.b - b.a).cross(b.c - b.a).try_normalize() else {
        return false;
    };
    let Some(direction) = na.cross(nb).try_normalize() else {
        return false;
    };
    let slice = |triangle: &Triangle, origin: Vector, normal: Vector| -> Option<(f64, f64)> {
        let points = [triangle.a, triangle.b, triangle.c];
        let distances = points.map(|point| (point - origin).dot(normal));
        if !distances.iter().any(|d| *d < -EPS) || !distances.iter().any(|d| *d > EPS) {
            return None;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in 0..3 {
            let j = (i + 1) % 3;
            let point = if distances[i].abs() <= EPS {
                Some(points[i])
            } else if (distances[i] < 0.) != (distances[j] < 0.) {
                Some(
                    points[i]
                        + (points[j] - points[i]) * (distances[i] / (distances[i] - distances[j])),
                )
            } else {
                None
            };
            if let Some(point) = point {
                let coordinate = point.dot(direction);
                lo = lo.min(coordinate);
                hi = hi.max(coordinate);
            }
        }
        Some((lo, hi))
    };
    match (slice(a, b.a, nb), slice(b, a.a, na)) {
        (Some((a0, a1)), Some((b0, b1))) => a1.min(b1) - a0.max(b0) > EPS,
        _ => false,
    }
}

/// Penetration depth from vertices: for the given world-space vertices of
/// one mesh, the distance to the other mesh's surface of those that lie
/// inside it; the maximum over both directions. Zero for touching coplanar
/// faces. Edge-edge crossings without interior vertices are underestimated.
pub fn penetration_depth(
    vertices_a: &[Vector],
    b: &TriMesh,
    pose_b: &Pose,
    vertices_b: &[Vector],
    a: &TriMesh,
    pose_a: &Pose,
) -> f64 {
    let mut depth = 0.0f64;
    for (points, other, pose) in [(vertices_a, b, pose_b), (vertices_b, a, pose_a)] {
        for p in points {
            if other.contains_point(pose, *p) {
                let projection = other.project_point(pose, *p, false);
                depth = depth.max((projection.point - *p).length());
            }
        }
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_crossing_excludes_coplanar_and_disjoint_triangles() {
        let a = Triangle::new(
            Vector::new(-1., -1., 0.),
            Vector::new(1., -1., 0.),
            Vector::new(0., 1., 0.),
        );
        let b = Triangle::new(
            Vector::new(0., -0.5, -1.),
            Vector::new(0., -0.5, 1.),
            Vector::new(0., 0.5, 0.),
        );
        assert!(crosses(&a, &b));
        assert!(crosses(&b, &a));
        assert!(!crosses(&a, &a));
        let distant = Triangle::new(
            b.a + Vector::Y * 5.,
            b.b + Vector::Y * 5.,
            b.c + Vector::Y * 5.,
        );
        assert!(!crosses(&a, &distant));
        let touches = Triangle::new(
            Vector::new(0., -0.5, 0.),
            Vector::new(0., 0.5, 0.),
            Vector::new(0., 0., 1.),
        );
        assert!(!crosses(&a, &touches));
    }
}
