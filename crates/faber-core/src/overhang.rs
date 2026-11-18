//! Overhang analysis for 3D printing. Each triangle gets a class from its angle to the build direction.
//! It never changes a solid.

use faber_ports::{Mesh, Result};
use glam::DVec3;
use std::collections::{BTreeMap, HashMap};

/// Most regions in a report.
const MAX_REGIONS: usize = 5;
/// A bed face has all corners this close to the lowest point.
const BED_HEIGHT_MM: f64 = 0.05;
/// A bed face points this close to the opposite of the build direction.
const BED_NORMAL_DEG: f64 = 0.5;
/// Tolerance for the critical angle test.
const ANGLE_EPSILON_DEG: f64 = 1e-9;
const ORANGE: [u8; 3] = [255, 140, 0];
const RED: [u8; 3] = [200, 20, 20];
const BED_COLOUR: [u8; 3] = [30, 110, 230];

/// The class of one triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Facing {
    /// Needs no support.
    Safe,
    /// Rests on the bed. Needs no support.
    Bed,
    /// Needs support. The angle is from the downward direction. Zero is a flat underside.
    /// Severity is 1 when flat and 0 at the critical angle.
    Support { angle_deg: f64, severity: f64 },
}

/// A connected group of support triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// Index of the mesh in the list that `analyse` received.
    pub part: usize,
    pub centroid_mm: [f64; 3],
    pub area_mm2: f64,
    pub triangles: usize,
}

/// Areas and the largest support regions of one or more meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub total_area_mm2: f64,
    pub support_area_mm2: f64,
    pub bed_area_mm2: f64,
    /// The smallest angle from the downward direction over support faces.
    /// Absent when no face needs support.
    pub flattest_overhang_deg: Option<f64>,
    /// Number of support regions in all meshes.
    pub regions_total: usize,
    /// Largest regions first. At most five.
    pub regions: Vec<Region>,
}

/// Classes each triangle of one mesh. A degenerate triangle is safe.
pub fn classify(mesh: &Mesh, direction: [f64; 3], critical_deg: f64) -> Result<Vec<Facing>> {
    let (up, critical) = settings(direction, critical_deg)?;
    check(mesh)?;
    let lowest = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from(*v).dot(up))
        .fold(f64::INFINITY, f64::min);
    let mut facings = Vec::with_capacity(mesh.triangles.len());
    for triangle in &mesh.triangles {
        let points = corners(mesh, triangle);
        let cross = (points[1] - points[0]).cross(points[2] - points[0]);
        let length = cross.length();
        if length == 0. {
            facings.push(Facing::Safe);
            continue;
        }
        let angle = (cross / length).dot(-up).clamp(-1., 1.).acos().to_degrees();
        let bed =
            angle <= BED_NORMAL_DEG && points.iter().all(|p| p.dot(up) <= lowest + BED_HEIGHT_MM);
        facings.push(if bed {
            Facing::Bed
        } else if angle <= critical + ANGLE_EPSILON_DEG {
            let severity = if critical > 0. {
                (1. - angle / critical).clamp(0., 1.)
            } else {
                1.
            };
            Facing::Support {
                angle_deg: angle,
                severity,
            }
        } else {
            Facing::Safe
        });
    }
    Ok(facings)
}

/// Reports the areas and support regions of the meshes. Each mesh has its own bed.
pub fn analyse(meshes: &[&Mesh], direction: [f64; 3], critical_deg: f64) -> Result<Report> {
    let mut report = Report {
        total_area_mm2: 0.,
        support_area_mm2: 0.,
        bed_area_mm2: 0.,
        flattest_overhang_deg: None,
        regions_total: 0,
        regions: Vec::new(),
    };
    let mut regions = Vec::new();
    for (part, mesh) in meshes.iter().enumerate() {
        let facings = classify(mesh, direction, critical_deg)?;
        for (triangle, facing) in mesh.triangles.iter().zip(&facings) {
            let area = triangle_area(mesh, triangle);
            report.total_area_mm2 += area;
            match facing {
                Facing::Safe => {}
                Facing::Bed => report.bed_area_mm2 += area,
                Facing::Support { angle_deg, .. } => {
                    report.support_area_mm2 += area;
                    report.flattest_overhang_deg = Some(
                        report
                            .flattest_overhang_deg
                            .map_or(*angle_deg, |least| least.min(*angle_deg)),
                    );
                }
            }
        }
        let found = support_regions(mesh, &facings, part);
        report.regions_total += found.len();
        regions.extend(found);
    }
    regions.sort_by(|a, b| b.area_mm2.total_cmp(&a.area_mm2));
    regions.truncate(MAX_REGIONS);
    report.regions = regions;
    Ok(report)
}

/// Returns one colour per triangle. Support goes from orange at the critical angle to red when flat.
pub fn paint(facings: &[Facing], base: [u8; 3]) -> Vec<[u8; 3]> {
    facings
        .iter()
        .map(|facing| match *facing {
            Facing::Safe => base,
            Facing::Bed => BED_COLOUR,
            Facing::Support { severity, .. } => mix(ORANGE, RED, severity),
        })
        .collect()
}

fn mix(from: [u8; 3], to: [u8; 3], t: f64) -> [u8; 3] {
    std::array::from_fn(|i| (from[i] as f64 + (to[i] as f64 - from[i] as f64) * t).round() as u8)
}

fn settings(direction: [f64; 3], critical_deg: f64) -> Result<(DVec3, f64)> {
    let up = DVec3::from(direction);
    if !(up.is_finite() && up.length() > 0.) {
        return Err("Direction must be a finite vector that is not zero".into());
    }
    if !(0. ..=90.).contains(&critical_deg) {
        return Err("Critical angle must be from 0 to 90 degrees".into());
    }
    Ok((up.normalize(), critical_deg))
}

fn check(mesh: &Mesh) -> Result<()> {
    if mesh.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Mesh vertex must be finite".into());
    }
    let count = mesh.vertices.len();
    if mesh
        .triangles
        .iter()
        .flatten()
        .any(|&i| i as usize >= count)
    {
        return Err("Mesh triangle refers to a missing vertex".into());
    }
    Ok(())
}

fn corners(mesh: &Mesh, triangle: &[u32; 3]) -> [DVec3; 3] {
    triangle.map(|i| DVec3::from(mesh.vertices[i as usize]))
}

fn triangle_area(mesh: &Mesh, triangle: &[u32; 3]) -> f64 {
    let points = corners(mesh, triangle);
    0.5 * (points[1] - points[0])
        .cross(points[2] - points[0])
        .length()
}

/// Maps each vertex to the first vertex at the same position, to 1e-6 mm.
/// Faces that meet at a seam then share an edge.
fn welded(mesh: &Mesh) -> Vec<u32> {
    let mut first: HashMap<[i64; 3], u32> = HashMap::new();
    mesh.vertices
        .iter()
        .enumerate()
        .map(|(index, v)| {
            let key = v.map(|c| (c * 1e6).round() as i64);
            *first.entry(key).or_insert(index as u32)
        })
        .collect()
}

/// Groups support triangles that share an edge.
fn support_regions(mesh: &Mesh, facings: &[Facing], part: usize) -> Vec<Region> {
    let support: Vec<usize> = facings
        .iter()
        .enumerate()
        .filter(|(_, facing)| matches!(facing, Facing::Support { .. }))
        .map(|(index, _)| index)
        .collect();
    let weld = welded(mesh);
    let mut parent: Vec<usize> = (0..mesh.triangles.len()).collect();
    let mut edges: HashMap<(u32, u32), usize> = HashMap::new();
    for &index in &support {
        let triangle = mesh.triangles[index];
        for k in 0..3 {
            let a = weld[triangle[k] as usize];
            let b = weld[triangle[(k + 1) % 3] as usize];
            let edge = (a.min(b), a.max(b));
            match edges.get(&edge) {
                Some(&other) => {
                    let (x, y) = (find(&mut parent, index), find(&mut parent, other));
                    parent[x] = y;
                }
                None => {
                    edges.insert(edge, index);
                }
            }
        }
    }
    // Per root: area, area-weighted centroid sum and triangle count.
    let mut groups: BTreeMap<usize, (f64, DVec3, usize)> = BTreeMap::new();
    for &index in &support {
        let root = find(&mut parent, index);
        let triangle = &mesh.triangles[index];
        let area = triangle_area(mesh, triangle);
        let centre = corners(mesh, triangle).iter().sum::<DVec3>() / 3.;
        let entry = groups.entry(root).or_insert((0., DVec3::ZERO, 0));
        entry.0 += area;
        entry.1 += centre * area;
        entry.2 += 1;
    }
    groups
        .into_values()
        .map(|(area, weighted, triangles)| Region {
            part,
            centroid_mm: (weighted / area).to_array(),
            area_mm2: area,
            triangles,
        })
        .collect()
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adds a quad and orients its two triangles along `outward`.
    fn quad(mesh: &mut Mesh, corners: [[f64; 3]; 4], outward: [f64; 3]) {
        let start = mesh.vertices.len() as u32;
        mesh.vertices.extend(corners);
        let (a, b, c) = (
            DVec3::from(corners[0]),
            DVec3::from(corners[1]),
            DVec3::from(corners[2]),
        );
        let flip = (b - a).cross(c - a).dot(DVec3::from(outward)) < 0.;
        let tris = if flip {
            [[0, 2, 1], [0, 3, 2]]
        } else {
            [[0, 1, 2], [0, 2, 3]]
        };
        for t in tris {
            mesh.triangles.push(t.map(|i| start + i));
        }
    }

    fn cube(size: f64) -> Mesh {
        let mut mesh = Mesh::default();
        let s = size;
        let faces: [([[f64; 3]; 4], [f64; 3]); 6] = [
            (
                [[0., 0., 0.], [s, 0., 0.], [s, s, 0.], [0., s, 0.]],
                [0., 0., -1.],
            ),
            (
                [[0., 0., s], [s, 0., s], [s, s, s], [0., s, s]],
                [0., 0., 1.],
            ),
            (
                [[0., 0., 0.], [s, 0., 0.], [s, 0., s], [0., 0., s]],
                [0., -1., 0.],
            ),
            (
                [[0., s, 0.], [s, s, 0.], [s, s, s], [0., s, s]],
                [0., 1., 0.],
            ),
            (
                [[0., 0., 0.], [0., s, 0.], [0., s, s], [0., 0., s]],
                [-1., 0., 0.],
            ),
            (
                [[s, 0., 0.], [s, s, 0.], [s, s, s], [s, 0., s]],
                [1., 0., 0.],
            ),
        ];
        for (corners, outward) in faces {
            quad(&mut mesh, corners, outward);
        }
        mesh
    }

    /// Extrudes a profile in X and Z along Y from 0 to `depth`. The profile runs counter-clockwise.
    fn prism(profile: &[[f64; 2]], depth: f64) -> Mesh {
        let mut mesh = Mesh::default();
        for k in 0..profile.len() {
            let a = profile[k];
            let b = profile[(k + 1) % profile.len()];
            let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
            let outward = [ez, 0., -ex];
            quad(
                &mut mesh,
                [
                    [a[0], 0., a[1]],
                    [b[0], 0., b[1]],
                    [b[0], depth, b[1]],
                    [a[0], depth, a[1]],
                ],
                outward,
            );
        }
        mesh
    }

    /// Two pillars under a bar. The bar underside is the only overhang.
    fn bridge() -> Mesh {
        prism(
            &[
                [0., 0.],
                [10., 0.],
                [10., 20.],
                [30., 20.],
                [30., 0.],
                [40., 0.],
                [40., 30.],
                [0., 30.],
            ],
            10.,
        )
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn cube_rests_on_the_bed_with_no_overhang() {
        let report = analyse(&[&cube(10.)], [0., 0., 1.], 45.).unwrap();
        assert_near(report.total_area_mm2, 600.);
        assert_near(report.bed_area_mm2, 100.);
        assert_near(report.support_area_mm2, 0.);
        assert_eq!(report.flattest_overhang_deg, None);
        assert_eq!(report.regions_total, 0);
    }

    #[test]
    fn bridge_underside_is_the_only_support() {
        let mesh = bridge();
        let report = analyse(&[&mesh], [0., 0., 1.], 45.).unwrap();
        // Pillar bottoms are on the bed. The bar underside is 20 by 10.
        assert_near(report.bed_area_mm2, 200.);
        assert_near(report.support_area_mm2, 200.);
        assert_near(report.flattest_overhang_deg.unwrap(), 0.);
        assert_eq!(report.regions_total, 1);
        let region = &report.regions[0];
        assert_eq!(region.triangles, 2);
        assert_near(region.area_mm2, 200.);
        for (got, want) in region.centroid_mm.iter().zip([20., 5., 20.]) {
            assert_near(*got, want);
        }
    }

    #[test]
    fn chamfer_at_exactly_the_critical_angle_needs_support() {
        // The chamfer runs from (10, 0) to (20, 10), so its face is 45 degrees from the bed.
        let mesh = prism(
            &[[0., 0.], [10., 0.], [20., 10.], [20., 20.], [0., 20.]],
            10.,
        );
        let at_limit = analyse(&[&mesh], [0., 0., 1.], 45.).unwrap();
        assert_near(at_limit.support_area_mm2, 10. * 2f64.sqrt() * 10.);
        assert_near(at_limit.flattest_overhang_deg.unwrap(), 45.);
        let below = analyse(&[&mesh], [0., 0., 1.], 44.).unwrap();
        assert_near(below.support_area_mm2, 0.);
        assert_eq!(below.flattest_overhang_deg, None);
    }

    #[test]
    fn paint_colours_each_class() {
        let facings = [
            Facing::Safe,
            Facing::Bed,
            Facing::Support {
                angle_deg: 0.,
                severity: 1.,
            },
            Facing::Support {
                angle_deg: 45.,
                severity: 0.,
            },
        ];
        let colours = paint(&facings, [10, 20, 30]);
        assert_eq!(colours, vec![[10, 20, 30], BED_COLOUR, RED, ORANGE]);
    }

    #[test]
    fn invalid_settings_are_refused() {
        let mesh = cube(10.);
        assert!(classify(&mesh, [0., 0., 0.], 45.).is_err());
        assert!(classify(&mesh, [0., 0., 1.], 91.).is_err());
        assert!(classify(&mesh, [0., 0., 1.], f64::NAN).is_err());
    }

    #[test]
    fn regions_join_across_face_seams() {
        // Two underside quads meet at x = 10. Their seam vertices have no shared index.
        // A bed quad lies below them.
        let mut mesh = Mesh::default();
        quad(
            &mut mesh,
            [[0., 0., 5.], [10., 0., 5.], [10., 10., 5.], [0., 10., 5.]],
            [0., 0., -1.],
        );
        quad(
            &mut mesh,
            [[10., 0., 5.], [20., 0., 5.], [20., 10., 5.], [10., 10., 5.]],
            [0., 0., -1.],
        );
        quad(
            &mut mesh,
            [
                [-50., 0., 0.],
                [-40., 0., 0.],
                [-40., 10., 0.],
                [-50., 10., 0.],
            ],
            [0., 0., -1.],
        );
        let report = analyse(&[&mesh], [0., 0., 1.], 45.).unwrap();
        assert_eq!(report.regions_total, 1);
        assert_near(report.regions[0].area_mm2, 200.);
    }

    #[test]
    fn non_finite_vertex_is_refused() {
        let mut mesh = cube(10.);
        mesh.vertices[0][1] = f64::NAN;
        assert!(classify(&mesh, [0., 0., 1.], 45.).is_err());
        assert!(analyse(&[&mesh], [0., 0., 1.], 45.).is_err());
    }

    #[test]
    fn missing_vertex_is_refused() {
        let mesh = Mesh {
            vertices: vec![[0., 0., 0.]],
            triangles: vec![[0, 1, 2]],
            normals: Vec::new(),
        };
        assert!(classify(&mesh, [0., 0., 1.], 45.).is_err());
    }
}
