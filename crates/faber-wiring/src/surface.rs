//! Read surface measurements in assembly coordinates.
#[path = "surface_sampling.rs"]
pub mod sampling;
use faber_core::{Project, surface as domain};
use faber_geometry::bvh::Bvh;
use faber_kernel_occt::OcctKernel;
use faber_ports::{Body, CurvedMesh, FaceMesh, Kernel, Mesh, Result};
use glam::DVec3;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize)]
pub enum Request {
    Curvature {
        mode: domain::CurvatureMode,
        tool_radius_mm: Option<f64>,
    },
    Draft {
        pull: [f64; 3],
        min_deg: f64,
    },
    Access {
        directions: Vec<[f64; 3]>,
        scene: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub mesh: FaceMesh,
    pub colours: Vec<[u8; 3]>,
    pub range: Option<[f64; 2]>,
    pub report: Value,
}

pub(crate) const DEFAULT_CHORD_MM: f64 = 0.1;
const MAX_TRIANGLES: usize = 500_000;
const MAX_SCENE_TRIANGLES: usize = 1_500_000;

fn check_size(n: usize, limit: usize) -> Result<()> {
    if n > limit {
        return Err(format!(
            "Surface analysis has {n} triangles. The limit is {limit}."
        ));
    }
    Ok(())
}

fn face_mesh(body: &Body, chord: f64) -> Result<FaceMesh> {
    let mut result = match &body.shape {
        Some(shape) => OcctKernel.tessellate_faces(shape, chord)?,
        None => FaceMesh {
            mesh: body.mesh.clone(),
            face_of_triangle: body.face_of_triangle.clone(),
        },
    };
    check_size(result.mesh.triangles.len(), MAX_TRIANGLES)?;
    if result.face_of_triangle.is_empty() {
        result
            .face_of_triangle
            .resize(result.mesh.triangles.len(), 1);
    }
    Ok(result)
}

fn obstacles(
    bodies: &[Body],
    selected: &str,
    chord: f64,
    poses: &std::collections::BTreeMap<String, glam::Mat4>,
) -> Result<Option<Bvh>> {
    let mut scene = Mesh::default();
    for body in bodies.iter().filter(|b| b.id != selected) {
        check_size(
            scene
                .triangles
                .len()
                .saturating_add(body.mesh.triangles.len()),
            MAX_SCENE_TRIANGLES,
        )?;
        let mut faces = face_mesh(body, chord)?;
        if let Some(&transform) = poses.get(&body.id)
            && transform != glam::Mat4::IDENTITY
        {
            crate::surface_pose::transform_mesh(&mut faces.mesh, transform);
        }
        check_size(
            scene.triangles.len() + faces.mesh.triangles.len(),
            MAX_SCENE_TRIANGLES,
        )?;
        let start =
            u32::try_from(scene.vertices.len()).map_err(|_| "The scene has too many vertices.")?;
        scene.vertices.extend(faces.mesh.vertices);
        scene.triangles.extend(
            faces
                .mesh
                .triangles
                .into_iter()
                .map(|t| t.map(|i| start + i)),
        );
    }
    if scene.triangles.is_empty() {
        Ok(None)
    } else {
        Bvh::new(&scene).map(Some)
    }
}

fn paint_triangles(
    faces: FaceMesh,
    colours: impl Iterator<Item = [u8; 3]>,
    report: Value,
) -> Analysis {
    let mut mesh = Mesh::default();
    let mut painted = Vec::with_capacity(faces.mesh.triangles.len() * 3);
    for (i, (triangle, colour)) in faces.mesh.triangles.iter().zip(colours).enumerate() {
        for &index in triangle {
            mesh.vertices.push(faces.mesh.vertices[index as usize]);
            if let Some(n) = faces.mesh.normals.get(index as usize) {
                mesh.normals.push(*n);
            }
            painted.push(colour);
        }
        let start = 3 * i as u32;
        mesh.triangles.push([start, start + 1, start + 2]);
    }
    Analysis {
        mesh: FaceMesh {
            mesh,
            face_of_triangle: faces.face_of_triangle,
        },
        colours: painted,
        range: None,
        report,
    }
}

pub fn analyse(bodies: &[Body], part: &str, request: &Request) -> Result<Analysis> {
    analyse_at(bodies, part, request, DEFAULT_CHORD_MM)
}

pub fn analyse_at(bodies: &[Body], part: &str, request: &Request, chord: f64) -> Result<Analysis> {
    analyse_in_frame(bodies, part, request, chord, &Default::default())
}

pub(crate) fn analyse_in_frame(
    bodies: &[Body],
    part: &str,
    request: &Request,
    chord: f64,
    poses: &std::collections::BTreeMap<String, glam::Mat4>,
) -> Result<Analysis> {
    if !(0.001..=1.0).contains(&chord) {
        return Err(format!("Resolution {chord} mm is outside 0.001 to 1.0 mm."));
    }
    let body = bodies
        .iter()
        .find(|b| b.id == part)
        .ok_or_else(|| format!("Unknown part: {part}"))?;
    check_size(body.mesh.triangles.len(), MAX_TRIANGLES)?;
    let mut sample_edge = None;
    let (mut analysis, method) = match request {
        Request::Curvature {
            mode,
            tool_radius_mm,
        } => {
            let (curved, topology, method) = if let Some(shape) = &body.shape {
                (
                    OcctKernel.tessellate_curvature(shape, chord)?,
                    OcctKernel.faces(shape)?,
                    "Principal curvature of the exact surface",
                )
            } else {
                let faces = face_mesh(body, chord)?;
                let curvature = domain::checked_estimate_curvature(&faces.mesh)?;
                (
                    CurvedMesh {
                        faces,
                        curvature,
                        kinds: Default::default(),
                    },
                    vec![],
                    "Curvature estimated from the part mesh",
                )
            };
            check_size(curved.faces.mesh.triangles.len(), MAX_TRIANGLES)?;
            let report = domain::checked_curvature_report(&curved, &topology)?;
            let mut value = json!(&report);
            if let Some(radius) = tool_radius_mm {
                let faces = report.tool_faces(*radius)?;
                value["tool_radius_mm"] = json!(radius);
                value["tool_too_large_faces"] = json!(faces);
            }
            let range = domain::curvature_range(&curved, *mode);
            let colours = domain::curvature_colours(&curved, *mode, Some(range));
            (
                Analysis {
                    mesh: curved.faces,
                    colours,
                    range: Some(range),
                    report: value,
                },
                method,
            )
        }
        Request::Draft { pull, min_deg } => {
            let source = face_mesh(body, chord)?;
            let (faces, edge) = sampling::refine(&source, chord)?;
            sample_edge = Some(edge);
            let original = crate::surface_pose::world_faces(&source, poses.get(part));
            let bvh = Bvh::new(&original.mesh)?;
            let samples = crate::surface_pose::world_faces(&faces, poses.get(part));
            let report = domain::draft_report_sampled(&samples, &original, *pull, *min_deg, &bvh)?;
            let value = json!(&report);
            let colours = report
                .triangle_classes
                .into_iter()
                .map(domain::draft_colour);
            (
                paint_triangles(faces, colours, value),
                "Triangle normals and rays along the pull axis",
            )
        }
        Request::Access { directions, scene } => {
            if !(1..=32).contains(&directions.len()) {
                return Err(format!(
                    "Access analysis has {} directions. Give 1 to 32.",
                    directions.len()
                ));
            }
            let source = face_mesh(body, chord)?;
            let own = Bvh::new(&crate::surface_pose::world_faces(&source, poses.get(part)).mesh)?;
            let (faces, edge) = sampling::refine(&source, chord)?;
            sample_edge = Some(edge);
            let world = crate::surface_pose::world_faces(&faces, poses.get(part));
            let others = if *scene {
                obstacles(bodies, part, chord, poses)?
            } else {
                None
            };
            let report = domain::checked_access_report(&world, &own, directions, others.as_ref())?;
            let value = json!(&report);
            let colours = report.reachable.into_iter().map(domain::access_colour);
            (
                paint_triangles(faces, colours, value),
                "Ray visibility from triangle centres",
            )
        }
    };
    analysis.report["part"] = json!(part);
    analysis.report["method"] = json!(method);
    analysis.report["resolution"] = json!({"triangles":analysis.mesh.mesh.triangles.len(),
        "chord_mm":body.shape.as_ref().map(|_|chord)});
    if let Some(edge) = sample_edge {
        analysis.report["resolution"]["max_sample_edge_mm"] = json!(edge);
        if let Some(limits) = analysis.report["limits"].as_array_mut() {
            limits.push(json!(
                "A feature smaller than the sample spacing can be missed."
            ));
        }
    }
    Ok(analysis)
}

pub fn cached(
    project: &Project,
    part: &str,
    request: &Request,
    chord: Option<f64>,
) -> Result<Arc<Analysis>> {
    let chord = chord.unwrap_or(DEFAULT_CHORD_MM);
    let key = serde_json::to_string(&(part, request, chord)).map_err(|e| e.to_string())?;
    // A direction or radius can change on every call. Keep one large map per project.
    let slot = project.cached("surface_analysis", "", || {
        Ok(Arc::new(Mutex::new(None::<(String, Arc<Analysis>)>)))
    })?;
    let mut last = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((saved, field)) = last.as_ref()
        && saved == &key
    {
        return Ok(Arc::clone(field));
    }
    let result = Arc::new(analyse_at(&project.bodies, part, request, chord)?);
    *last = Some((key, Arc::clone(&result)));
    Ok(result)
}

pub fn centre_of_mass(body: &Body) -> Result<[f64; 3]> {
    if let Some(shape) = &body.shape {
        return Ok(OcctKernel.mass_properties(shape)?.centre_of_mass_mm);
    }
    if !faber_meshtools::try_report(&body.mesh)?.watertight {
        return Err("The centre of mass needs a closed and consistently oriented mesh.".into());
    }
    let origin = DVec3::from(body.mesh.vertices[0]);
    let mut volume = 0.;
    let mut moment = DVec3::ZERO;
    for t in &body.mesh.triangles {
        let [a, b, c] = t.map(|i| DVec3::from(body.mesh.vertices[i as usize]) - origin);
        let weight = a.dot(b.cross(c));
        volume += weight;
        moment += (a + b + c) * weight;
    }
    let centre = origin + moment / (4. * volume);
    if !centre.is_finite() {
        return Err("The mesh has no finite centre of mass.".into());
    }
    Ok(centre.to_array())
}

pub fn save_image(body: &Body, field: &Analysis, path: &std::path::Path) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("png"))
    {
        return Err("Choose a PNG file for the surface map.".into());
    }
    let mut shown = body.clone();
    shown.mesh.clone_from(&field.mesh.mesh);
    shown
        .face_of_triangle
        .clone_from(&field.mesh.face_of_triangle);
    let image =
        faber_render::vertex_image(&shown, &field.colours, 900, 650, faber_scene::View::Iso)?;
    crate::study_image::save_png(&image, path)
}
