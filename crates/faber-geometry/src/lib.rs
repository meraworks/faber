//! Closed meshes from flat profiles and spatial queries on triangle meshes.

pub mod bvh;
pub mod sketch_loops;

use faber_model::{Geometry, Profile};
use faber_ports::{Kernel, Mesh, Result};

/// Measure a closed, consistently wound mesh with valid vertex indices. Units are cubic millimetres.
pub fn signed_volume(mesh: &Mesh) -> f64 {
    use glam::DVec3;
    let Some(first) = mesh.triangles.first() else {
        return 0.;
    };
    let origin = mesh.vertices[first[0] as usize];
    mesh.triangles
        .iter()
        .map(|tri| {
            let [a, b, c] =
                tri.map(|i| DVec3::from(mesh.vertices[i as usize]) - DVec3::from(origin));
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}

pub struct NativeKernel;

impl Kernel for NativeKernel {
    fn build(&self, geometry: &Geometry) -> Result<Mesh> {
        match geometry {
            Geometry::Instance { .. } => Err("An instance needs a source part.".into()),
            Geometry::Imported { .. }
            | Geometry::Thread { .. }
            | Geometry::Hole { .. }
            | Geometry::Split { .. }
            | Geometry::Mirror { .. }
            | Geometry::CircularPattern { .. }
            | Geometry::LinearPattern { .. }
            | Geometry::Curve { .. }
            | Geometry::Helix { .. }
            | Geometry::Plane { .. }
            | Geometry::PlaneOnFace { .. }
            | Geometry::Axis { .. }
            | Geometry::Point { .. }
            | Geometry::PlaneAtAngle { .. }
            | Geometry::Midplane { .. }
            | Geometry::PlaneThroughPoints { .. }
            | Geometry::AxisFromCylinder { .. }
            | Geometry::AxisFromPlanes { .. }
            | Geometry::PointAtCentre { .. }
            | Geometry::RevolveAround { .. }
            | Geometry::PlaceAtPoint { .. }
            | Geometry::SurfaceExtrude { .. }
            | Geometry::SurfaceRevolve { .. }
            | Geometry::SurfaceLoft { .. }
            | Geometry::SurfaceSweep { .. }
            | Geometry::Patch { .. }
            | Geometry::Trim { .. }
            | Geometry::Extend { .. }
            | Geometry::Stitch { .. }
            | Geometry::SplitBody { .. }
            | Geometry::ReplaceFace { .. }
            | Geometry::Form { .. }
            | Geometry::SheetMetal { .. }
            | Geometry::SolvedSketch { .. }
            | Geometry::OffsetSketch { .. }
            | Geometry::SketchOnPlane { .. }
            | Geometry::SweepSketch { .. } => Err("Operation needs a feature context".into()),
            Geometry::RevolveSketch { .. } | Geometry::LoftSketches { .. } => {
                Err("Operation needs a feature context".into())
            }
            Geometry::Sketch { .. } | Geometry::ExtrudeSketch { .. } => {
                Err("Operation needs a feature context".into())
            }
            Geometry::Shell { .. }
            | Geometry::Rib { .. }
            | Geometry::Web { .. }
            | Geometry::Boss { .. }
            | Geometry::SnapHook { .. }
            | Geometry::Lip { .. }
            | Geometry::Groove { .. }
            | Geometry::RemoveFaces { .. }
            | Geometry::MoveFaces { .. }
            | Geometry::SplitFace { .. }
            | Geometry::Emboss { .. }
            | Geometry::Offset { .. }
            | Geometry::Draft { .. }
            | Geometry::Thicken { .. } => Err("Operation needs a feature context".into()),
            Geometry::Fillet { .. } | Geometry::Chamfer { .. } => {
                Err("Operation needs a feature context".into())
            }
            Geometry::Sweep { .. } => Err("Operation needs exact solids".into()),
            Geometry::Loft { .. } => Err("Operation needs exact solids".into()),
            Geometry::Sphere { .. }
            | Geometry::Cone { .. }
            | Geometry::Torus { .. }
            | Geometry::Revolve { .. } => Err("Operation needs exact solids".into()),
            Geometry::Box { .. }
            | Geometry::Cylinder { .. }
            | Geometry::Transform { .. }
            | Geometry::Scale { .. }
            | Geometry::Pipe { .. }
            | Geometry::PathPattern { .. }
            | Geometry::Union { .. }
            | Geometry::Difference { .. }
            | Geometry::Intersection { .. }
            | Geometry::History { .. } => Err("Operation needs exact solids".into()),
            Geometry::Extrude { profile, height } => extrude(profile, *height, 0.),
            Geometry::Pocket {
                profile,
                height,
                floor,
            } => {
                if !floor.is_finite()
                    || *floor <= 0.
                    || *floor >= *height
                    || profile.holes.is_empty()
                {
                    return Err(
                        "Pocket needs a hole and a floor between zero and the height".into(),
                    );
                }
                extrude(profile, *height, *floor)
            }
        }
    }
}

const EPS: f64 = 1e-9;

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn area(ring: &[[f64; 2]]) -> f64 {
    (0..ring.len())
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        / 2.
}

fn on_segment(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    cross(a, b, p).abs() < EPS
        && (0..2).all(|i| p[i] >= a[i].min(b[i]) - EPS && p[i] <= a[i].max(b[i]) + EPS)
}

fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (ac, ad, ca, cb) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    (ac * ad < 0. && ca * cb < 0.)
        || on_segment(a, b, c)
        || on_segment(a, b, d)
        || on_segment(c, d, a)
        || on_segment(c, d, b)
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut hit = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            hit = !hit;
        }
    }
    hit
}

fn validate(profile: &Profile) -> Result<()> {
    let rings: Vec<_> = std::iter::once(&profile.outer)
        .chain(&profile.holes)
        .collect();
    if rings.iter().map(|r| r.len()).sum::<usize>() > 20_000 {
        return Err("Profile has more than 20000 points".into());
    }
    for ring in &rings {
        if ring.len() < 3
            || ring
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            || area(ring).abs() < EPS
        {
            return Err("Profile must have a finite area and at least three points".into());
        }
        for i in 0..ring.len() {
            let next = (i + 1) % ring.len();
            if (ring[i][0] - ring[next][0]).hypot(ring[i][1] - ring[next][1]) < EPS {
                return Err("Profile has a zero length edge".into());
            }
            for j in i + 1..ring.len() {
                if j == next || (j + 1) % ring.len() == i {
                    continue;
                }
                if intersects(ring[i], ring[next], ring[j], ring[(j + 1) % ring.len()]) {
                    return Err("Profile edges intersect".into());
                }
            }
        }
    }
    for i in 0..rings.len() {
        for j in i + 1..rings.len() {
            for a in 0..rings[i].len() {
                for b in 0..rings[j].len() {
                    if intersects(
                        rings[i][a],
                        rings[i][(a + 1) % rings[i].len()],
                        rings[j][b],
                        rings[j][(b + 1) % rings[j].len()],
                    ) {
                        return Err("Profile boundaries touch or intersect".into());
                    }
                }
            }
            if (i == 0 && !inside(rings[j][0], rings[0]))
                || (i > 0 && (inside(rings[j][0], rings[i]) || inside(rings[i][0], rings[j])))
            {
                return Err("Holes must be separate and inside the outer boundary".into());
            }
        }
    }
    Ok(())
}

fn extrude(profile: &Profile, height: f64, floor: f64) -> Result<Mesh> {
    if !height.is_finite() || !(1e-6..=1e6).contains(&height) {
        return Err("Height must be between 0.000001 and 1000000 mm".into());
    }
    validate(profile)?;
    let mut rings = Vec::new();
    for (i, ring) in std::iter::once(&profile.outer)
        .chain(&profile.holes)
        .enumerate()
    {
        let mut ring = ring.clone();
        loop {
            let redundant = (0..ring.len()).find(|&j| {
                on_segment(
                    ring[(j + ring.len() - 1) % ring.len()],
                    ring[(j + 1) % ring.len()],
                    ring[j],
                )
            });
            if let Some(j) = redundant {
                ring.remove(j);
            } else {
                break;
            }
            if ring.len() < 3 {
                return Err("Profile has no area after removing straight edge points".into());
            }
        }
        if (area(&ring) > 0.) != (i == 0) {
            ring.reverse();
        }
        rings.push(ring);
    }
    let mut points = Vec::new();
    let mut holes = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if i > 0 {
            holes.push(points.len());
        }
        points.extend(ring);
    }
    let flat: Vec<f64> = points
        .iter()
        .flat_map(|p: &&[f64; 2]| p.iter().copied())
        .collect();
    let cap = earcutr::earcut(&flat, &holes, 2).map_err(|e| format!("Cannot fill profile: {e}"))?;
    let n = points.len() as u32;
    let mut mesh = Mesh::default();
    mesh.vertices.extend(
        points
            .iter()
            .enumerate()
            .map(|(i, p)| [p[0], p[1], if i < rings[0].len() { 0. } else { floor }]),
    );
    mesh.vertices
        .extend(points.iter().map(|p| [p[0], p[1], height]));
    for t in cap.as_chunks::<3>().0 {
        let (a, mut b, mut c) = (t[0] as u32, t[1] as u32, t[2] as u32);
        if cross(
            *points[a as usize],
            *points[b as usize],
            *points[c as usize],
        ) < 0.
        {
            std::mem::swap(&mut b, &mut c);
        }
        if floor == 0. {
            mesh.triangles.push([a, c, b]);
        }
        mesh.triangles.push([a + n, b + n, c + n]);
    }
    if floor > 0. {
        let mut offset = 0u32;
        for (i, ring) in rings.iter().enumerate() {
            let flat: Vec<f64> = ring.iter().flatten().copied().collect();
            let tris = earcutr::earcut(&flat, &[], 2).map_err(|e| e.to_string())?;
            for t in tris.as_chunks::<3>().0 {
                let (a, mut b, mut c) = (t[0], t[1], t[2]);
                let positive = cross(ring[a], ring[b], ring[c]) > 0.;
                if positive != (i > 0) {
                    std::mem::swap(&mut b, &mut c);
                }
                mesh.triangles
                    .push([offset + a as u32, offset + b as u32, offset + c as u32]);
            }
            offset += ring.len() as u32;
        }
    }
    let mut offset = 0;
    for ring in &rings {
        for i in 0..ring.len() as u32 {
            let a = offset + i;
            let b = offset + (i + 1) % ring.len() as u32;
            mesh.triangles.extend([[a, b, b + n], [a, b + n, a + n]]);
        }
        offset += ring.len() as u32;
    }
    Ok(mesh)
}

pub mod projected_lines;
