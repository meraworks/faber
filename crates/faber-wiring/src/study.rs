//! Static stress in a placed solid. Selectors use part coordinates.
use faber_core::{Project, TopologyKind};
use faber_geometry::bvh::Bvh;
use faber_model::{
    MaterialClass, MaterialData, Scalar,
    study::{FixtureKind, Study, StudyLoad},
};
use faber_ports::{Body, Mesh, Result};
use faber_sim::{
    Elastic, Grid, Load, StaticCase, StaticResult, Support, default_h, solve_static_with_progress,
    voxelise,
};
use glam::DVec3;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;

mod surface;
#[cfg(test)]
mod surface_tests;

#[derive(Clone, Debug, Serialize)]
pub struct HotSpot {
    pub point: [f64; 3],
    pub von_mises_mpa: f64,
    pub safety_factor: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct StressReport {
    pub study: String,
    pub part: String,
    pub material: String,
    pub max_von_mises_mpa: f64,
    pub max_displacement_mm: f64,
    pub min_safety_factor: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_safety_factor: Option<f64>,
    pub hot_spots: Vec<HotSpot>,
    pub reaction_n: [f64; 3],
    pub mass_g: f64,
    pub method: String,
    pub resolution_mm: f64,
    pub cells: usize,
    pub iterations: usize,
    pub residual: f64,
    pub limits: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct StudyField {
    pub vertex_von_mises: Vec<f32>,
    pub vertex_displacement: Vec<[f32; 3]>,
    /// Additional surface samples for the display.
    pub surface: Option<Arc<StudySurface>>,
}

#[derive(Clone, Debug)]
pub struct StudySurface {
    pub mesh: Mesh,
    pub face_of_triangle: Vec<u32>,
    pub vertex_von_mises: Vec<f32>,
    pub vertex_displacement: Vec<[f32; 3]>,
}

type Answer = (StressReport, StudyField);

pub fn run_stress(project: &Project, id: &str, resolution_mm: Option<f64>) -> Result<Answer> {
    run_stress_with_progress(project, id, resolution_mm, |_, _| true)
}
pub fn run_stress_with_progress(
    project: &Project,
    id: &str,
    resolution_mm: Option<f64>,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<Answer> {
    let study = project
        .data
        .studies
        .get(id)
        .ok_or_else(|| format!("Unknown stress study: {id}"))?;
    let key = format!("{id}/{:?}", resolution_mm.map(f64::to_bits));
    check_progress(&mut progress)?;
    project.cached("stress", &key, || {
        analyse(project, id, study, resolution_mm, progress)
    })
}
pub fn quick_stress(
    project: &Project,
    study: &Study<Scalar>,
    resolution_mm: Option<f64>,
) -> Result<Answer> {
    quick_stress_with_progress(project, study, resolution_mm, |_, _| true)
}
pub fn quick_stress_with_progress(
    project: &Project,
    study: &Study<Scalar>,
    resolution_mm: Option<f64>,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<Answer> {
    check_progress(&mut progress)?;
    let key = serde_json::to_string(&(study, resolution_mm.map(f64::to_bits)))
        .map_err(|e| e.to_string())?;
    project.cached("quick_stress", &key, || {
        analyse(project, "quick", study, resolution_mm, progress)
    })
}
fn check_progress(progress: &mut impl FnMut(usize, f64) -> bool) -> Result<()> {
    if progress(0, 1.) {
        Ok(())
    } else {
        Err("Study stopped.".into())
    }
}
fn analyse(
    project: &Project,
    id: &str,
    recipe: &Study<Scalar>,
    resolution_mm: Option<f64>,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<Answer> {
    check_progress(&mut progress)?;
    let study = project.resolve_study(recipe)?;
    let part = project
        .data
        .parts
        .get(&study.part)
        .ok_or_else(|| format!("Unknown part: {}", study.part))?;
    let material_id = study
        .material
        .as_ref()
        .or(part.material.as_ref())
        .ok_or_else(|| {
            format!(
                "Part {} has no material. Set a material or give one in the study.",
                study.part
            )
        })?;
    let material = faber_core::materials::get(&project.data, material_id)?;
    let body = project
        .bodies
        .iter()
        .find(|b| b.id == study.part)
        .ok_or("The study part has no solid.")?;
    let h = resolution_mm
        .or(study.resolution_mm)
        .unwrap_or_else(|| default_h(body.bounds));
    if !h.is_finite() || h <= 0. {
        return Err("The resolution must be finite and greater than zero.".into());
    }
    if study.fixtures.is_empty() {
        return Err("The study has no fixture. Add a fixed face.".into());
    }
    if study.loads.is_empty() {
        return Err("Add a load to the study.".into());
    }
    let (supports, loads) = boundary(project, body, &study, &material)?;
    check_progress(&mut progress)?;
    let grid = voxelise(&body.mesh, &Bvh::new(&body.mesh)?, h)?;
    check_progress(&mut progress)?;
    let case = StaticCase {
        grid,
        material: Elastic {
            youngs_mpa: material.youngs_mpa,
            poisson: material.poisson,
        },
        supports,
        loads,
        tolerance: 1e-6,
        max_iterations: 20_000,
    };
    let result = solve_static_with_progress(&case, &mut progress)?;
    if !progress(result.iterations, result.residual) {
        return Err("Study stopped.".into());
    }
    let report = report(id, material_id, &study, &material, &case.grid, &result)?;
    let field = field(body, &case.grid, &result)?;
    Ok((report, field))
}

type Triangles = Vec<[[f64; 3]; 3]>;
fn triangles(body: &Body, ids: &[u32]) -> Result<Triangles> {
    let ids: BTreeSet<_> = ids.iter().copied().collect();
    let mut found = BTreeSet::new();
    let triangles: Vec<_> = body
        .mesh
        .triangles
        .iter()
        .zip(&body.face_of_triangle)
        .filter_map(|(tri, face)| {
            if ids.contains(face) {
                found.insert(*face);
                Some(tri.map(|i| body.mesh.vertices[i as usize]))
            } else {
                None
            }
        })
        .collect();
    if ids != found || triangles.is_empty() {
        return Err("A selected face has no surface triangles.".into());
    }
    Ok(triangles)
}
fn boundary(
    project: &Project,
    body: &Body,
    study: &Study,
    material: &MaterialData,
) -> Result<(Vec<Support>, Vec<Load>)> {
    let mut fixed = BTreeSet::<u32>::new();
    let mut supports = Vec::new();
    let mut loads = Vec::new();
    for fixture in &study.fixtures {
        let ids = project.unplaced_faces_matching(&study.part, &fixture.face)?;
        fixed.extend(ids.iter().copied());
        match fixture.kind {
            FixtureKind::Fixed => supports.push(Support::FixedFace {
                triangles: triangles(body, &ids)?,
            }),
            FixtureKind::Slide => {
                let faces = project
                    .selected_topology_kind(&study.part, None, TopologyKind::Faces, None, None)?
                    .1
                    .unwrap_or_default();
                for id in ids {
                    let normal = faces
                        .iter()
                        .find(|f| f.index == id)
                        .and_then(|f| f.normal)
                        .ok_or("A slide fixture needs a planar face.")?;
                    supports.push(Support::SlideFace {
                        triangles: triangles(body, &[id])?,
                        normal,
                    });
                }
            }
        }
    }
    for load in &study.loads {
        let face = match load {
            StudyLoad::Force { face, .. } | StudyLoad::Pressure { face, .. } => face,
            StudyLoad::Gravity { direction } => {
                loads.push(Load::Gravity {
                    direction: *direction,
                    density_g_cm3: material.density_g_cm3,
                });
                continue;
            }
        };
        let ids = project.unplaced_faces_matching(&study.part, face)?;
        if ids.iter().any(|i| fixed.contains(i)) {
            return Err("A load acts on a fixed face. Select a different face.".into());
        }
        let triangles = triangles(body, &ids)?;
        loads.push(match load {
            StudyLoad::Pressure { mpa, .. } => Load::Pressure {
                triangles,
                mpa: *mpa,
            },
            StudyLoad::Force { newtons, .. } => Load::Traction {
                triangles,
                newtons: *newtons,
            },
            StudyLoad::Gravity { .. } => unreachable!(),
        });
    }
    Ok((supports, loads))
}

fn coordinates(grid: &Grid, i: usize) -> [usize; 3] {
    [
        i % grid.n[0],
        (i / grid.n[0]) % grid.n[1],
        i / (grid.n[0] * grid.n[1]),
    ]
}
fn centre(grid: &Grid, id: usize) -> DVec3 {
    let ijk = coordinates(grid, id);
    DVec3::from(grid.origin) + DVec3::from(ijk.map(|x| x as f64 + 0.5)) * grid.h
}
fn report(
    id: &str,
    material_id: &str,
    study: &Study,
    material: &MaterialData,
    grid: &Grid,
    result: &StaticResult,
) -> Result<StressReport> {
    let mut hot = Vec::<HotSpot>::new();
    for _ in 0..5 {
        let best = grid
            .fill
            .iter()
            .zip(&result.von_mises)
            .enumerate()
            .filter(|&(_, (f, s))| *f > 0. && *s > 0.)
            .filter(|(i, _)| {
                hot.iter().all(|other| {
                    centre(grid, *i).distance_squared(other.point.into()) >= 9. * grid.h * grid.h
                })
            })
            .max_by(|a, b| a.1.1.total_cmp(b.1.1));
        let Some((i, (_, value))) = best else { break };
        let stress = f64::from(*value);
        hot.push(HotSpot {
            point: centre(grid, i).to_array(),
            von_mises_mpa: stress,
            safety_factor: finite_factor(material.yield_mpa, stress)?,
        });
    }
    let max_von_mises_mpa = hot
        .first()
        .ok_or("The study has no non-zero stress. A safety factor cannot be calculated.")?
        .von_mises_mpa;
    let max_displacement_mm = result
        .displacement
        .iter()
        .map(|p| DVec3::from(*p).length())
        .fold(0., f64::max);
    let mut limits = vec![
        "Linear elastic material. Small displacement.".to_owned(),
        "One part. No contact.".to_owned(),
        format!(
            "Voxel grid of {} mm: stress at a curved surface is approximate.",
            grid.h
        ),
    ];
    let mut layer_safety_factor = None;
    if material.class == MaterialClass::Polymer {
        let factor = material.layer_strength_factor;
        limits.push(format!(
            "Layer direction strength is yield x {factor:.3}. Material source: {}",
            material.source
        ));
        if let Some([x, y, z]) = study.print_direction {
            let peak = result
                .stress_mpa
                .iter()
                .zip(&grid.fill)
                .filter(|(_, fill)| **fill > 0.)
                .map(|(s, _)| {
                    (s[0] * x * x
                        + s[1] * y * y
                        + s[2] * z * z
                        + 2. * (s[3] * x * y + s[4] * y * z + s[5] * z * x))
                        .abs()
                })
                .fold(0., f64::max);
            if peak > 0. {
                layer_safety_factor = Some(finite_factor(material.yield_mpa * factor, peak)?);
            } else {
                limits.push("There is no normal stress along the print direction.".into());
            }
            limits.push(
                "Layer safety uses the magnitude of normal stress along the print direction."
                    .into(),
            );
        } else {
            limits.push("Set the print direction to use the layer strength.".into());
        }
    }
    let mass_g = grid.volume() * material.density_g_cm3 / 1000.;
    if !mass_g.is_finite() || !max_displacement_mm.is_finite() {
        return Err("The stress result exceeds the numeric range.".into());
    }
    Ok(StressReport {
        study: id.into(),
        part: study.part.clone(),
        material: material_id.into(),
        max_von_mises_mpa,
        max_displacement_mm,
        min_safety_factor: hot[0].safety_factor,
        layer_safety_factor,
        hot_spots: hot,
        reaction_n: result.reaction,
        mass_g,
        method: "Voxel grid with trilinear hexahedral elements and conjugate gradients.".into(),
        resolution_mm: grid.h,
        cells: grid.fill.iter().filter(|f| **f > 0.).count(),
        iterations: result.iterations,
        residual: result.residual,
        limits,
    })
}

fn finite_factor(strength: f64, stress: f64) -> Result<f64> {
    let factor = strength / stress;
    if factor.is_finite() {
        Ok(factor)
    } else {
        Err("The safety factor exceeds the numeric range.".into())
    }
}

// Find the nearest centre. Stop when each unvisited cell is farther away.
fn nearest_cell(grid: &Grid, point: DVec3) -> Option<usize> {
    let q = (point - DVec3::from(grid.origin)) / grid.h - DVec3::splat(0.5);
    let q = q.to_array();
    let mut lo = std::array::from_fn::<_, 3, _>(|i| {
        (q[i].round() as isize).clamp(0, grid.n[i] as isize - 1) as usize
    });
    let mut hi = lo;
    let mut previous = None;
    let mut best = None;
    let mut distance = f64::INFINITY;
    loop {
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let ijk = [x, y, z];
                    if previous.is_some_and(|(a, b): ([usize; 3], [usize; 3])| {
                        (0..3).all(|i| ijk[i] >= a[i] && ijk[i] <= b[i])
                    }) {
                        continue;
                    }
                    let id = x + grid.n[0] * (y + grid.n[1] * z);
                    if grid.fill[id] > 0. {
                        let d = (0..3).map(|i| (ijk[i] as f64 - q[i]).powi(2)).sum::<f64>();
                        if d < distance {
                            distance = d;
                            best = Some(id);
                        }
                    }
                }
            }
        }
        let mut next = f64::INFINITY;
        previous = Some((lo, hi));
        for i in 0..3 {
            if lo[i] > 0 {
                lo[i] -= 1;
                next = next.min((lo[i] as f64 - q[i]).powi(2));
            }
            if hi[i] + 1 < grid.n[i] {
                hi[i] += 1;
                next = next.min((hi[i] as f64 - q[i]).powi(2));
            }
        }
        if distance <= next {
            return best;
        }
    }
}

fn field(body: &Body, grid: &Grid, result: &StaticResult) -> Result<StudyField> {
    let detail = surface::refine(&body.mesh, &body.face_of_triangle, grid.h, 150_000);
    let vertices = detail
        .as_ref()
        .map_or(&body.mesh.vertices, |(mesh, _)| &mesh.vertices);
    let mut vertex_von_mises = Vec::with_capacity(vertices.len());
    let mut vertex_displacement = Vec::with_capacity(vertices.len());
    for vertex in vertices {
        let point = DVec3::from(*vertex);
        let cell = nearest_cell(grid, point).ok_or("The part has no active cells.")?;
        vertex_von_mises.push(result.von_mises[cell]);
        let ijk = coordinates(grid, cell);
        let t = ((point - DVec3::from(grid.origin)) / grid.h - DVec3::from(ijk.map(|v| v as f64)))
            .clamp(DVec3::ZERO, DVec3::ONE)
            .to_array();
        let mut displacement = DVec3::ZERO;
        for k in 0..8 {
            let ij = std::array::from_fn::<_, 3, _>(|i| (k >> i) & 1);
            let weight = (0..3)
                .map(|i| if ij[i] == 0 { 1. - t[i] } else { t[i] })
                .product::<f64>();
            let node = grid.node(ijk[0] + ij[0], ijk[1] + ij[1], ijk[2] + ij[2]);
            displacement += DVec3::from(result.displacement[node]) * weight;
        }
        let value = displacement.as_vec3();
        if !value.is_finite() {
            return Err("The displacement exceeds the display range.".into());
        }
        vertex_displacement.push(value.to_array());
    }
    if let Some((mesh, face_of_triangle)) = detail {
        let count = body.mesh.vertices.len();
        return Ok(StudyField {
            vertex_von_mises: vertex_von_mises[..count].to_vec(),
            vertex_displacement: vertex_displacement[..count].to_vec(),
            surface: Some(Arc::new(StudySurface {
                mesh,
                face_of_triangle,
                vertex_von_mises,
                vertex_displacement,
            })),
        });
    }
    Ok(StudyField {
        vertex_von_mises,
        vertex_displacement,
        surface: None,
    })
}
