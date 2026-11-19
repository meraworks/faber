//! Run a study and map its fields to the part surface.
use super::*;
use faber_model::study::{StudyKind, ThermalCondition};
use faber_sim::{modal, shape, thermal};
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Analysis {
    pub report: Value,
    pub fields: Vec<StudyField>,
    pub shape_mesh: Option<Arc<Mesh>>,
}

pub fn run_analysis(project: &Project, id: &str, resolution_mm: Option<f64>) -> Result<Analysis> {
    let recipe = project
        .data
        .studies
        .get(id)
        .ok_or_else(|| format!("Unknown study: {id}"))?;
    let key = format!("{id}/{:?}", resolution_mm.map(f64::to_bits));
    project.cached("study", &key, || {
        run(project, id, recipe, resolution_mm, false, |_, _, _| true)
    })
}

pub fn quick_analysis_with_progress(
    project: &Project,
    recipe: &Study<Scalar>,
    resolution_mm: Option<f64>,
    progress: impl FnMut(usize, f64, Option<&StudyField>) -> bool,
) -> Result<Analysis> {
    run(project, "quick", recipe, resolution_mm, true, progress)
}

fn run(
    project: &Project,
    id: &str,
    recipe: &Study<Scalar>,
    resolution_mm: Option<f64>,
    preview: bool,
    mut progress: impl FnMut(usize, f64, Option<&StudyField>) -> bool,
) -> Result<Analysis> {
    if matches!(recipe.kind, StudyKind::Static) {
        let (r, f) = analyse(project, id, recipe, resolution_mm, |i, v| {
            progress(i, v, None)
        })?;
        return Ok(Analysis {
            report: json!(r),
            fields: vec![f],
            shape_mesh: None,
        });
    }
    let Prepared {
        study,
        body,
        material_id,
        material,
        mut case,
    } = prepare(project, recipe, resolution_mm, &mut |i, v| {
        progress(i, v, None)
    })?;
    let h = case.grid.h;
    let mass = case.grid.volume() * material.density_g_cm3 / 1000.;
    let cells = case.grid.fill.iter().filter(|f| **f >= 0.02).count();
    let face = |s| triangles(body, &project.unplaced_faces_matching(&study.part, s)?);
    let mut limits =
        vec!["One part on a regular grid. Curved boundaries are approximate.".to_string()];
    let mut fields = Vec::new();
    let mut shape_mesh = None;
    let mut report_value = match &study.kind {
        StudyKind::Static => unreachable!(),
        StudyKind::Modal { modes } => {
            if case.supports.is_empty() {
                limits.push(modal::MODAL_FREE_LIMIT.to_string());
            }
            limits.push("Linear elastic vibration. No preload, damping or contact.".into());
            let m = modal::ModalCase {
                grid: case.grid,
                material: case.material,
                density_g_cm3: material.density_g_cm3,
                supports: case.supports,
                modes: *modes as usize,
            };
            let display = ModeDisplay::new(body, &m.grid, m.modes, || progress(0, 1., None))?;
            let solved = modal::solve_modal_with_progress(&m, |i, v| progress(i, v, None))?;
            let mut modes = Vec::new();
            for (i, mode) in solved.into_iter().enumerate() {
                if !progress(i, 0., None) {
                    return Err("Study stopped.".into());
                }
                let direction = (0..3)
                    .max_by(|&a, &b| mode.mass_share[a].total_cmp(&mode.mass_share[b]))
                    .unwrap();
                let axis = ["X", "Y", "Z"][direction];
                modes.push(json!({"frequency_hz":mode.frequency_hz,"direction":axis}));
                fields.push(display.field(&mode.shape, || progress(i, 0., None))?);
            }
            json!({"kind":"modal","method":"Lumped mass and block subspace iteration.","modes":modes,"mass_g":mass})
        }
        StudyKind::Thermal {
            conditions,
            reference_c,
            stress,
        } => {
            let bcs = conditions
                .iter()
                .map(|condition| {
                    Ok(match condition {
                        ThermalCondition::Temperature { face: s, celsius } => {
                            thermal::ThermalBc::TemperatureFace {
                                triangles: face(s)?,
                                celsius: *celsius,
                            }
                        }
                        ThermalCondition::Power { face: s, watts } => {
                            thermal::ThermalBc::PowerFace {
                                triangles: face(s)?,
                                watts: *watts,
                            }
                        }
                        ThermalCondition::Convection {
                            face: s,
                            w_m2k,
                            ambient_c,
                        } => thermal::ThermalBc::Convection {
                            triangles: face(s)?,
                            w_m2k: *w_m2k,
                            ambient_c: *ambient_c,
                        },
                    })
                })
                .collect::<Result<_>>()?;
            let tcase = thermal::ThermalCase {
                grid: case.grid,
                conductivity_w_mk: material.conductivity_w_mk,
                conditions: bcs,
            };
            let heat = thermal::solve_thermal_with_progress(&tcase, |i, v| progress(i, v, None))?;
            case.grid = tcase.grid;
            fields.push(sample_field(body, &case.grid, |_, nodes| {
                (
                    (nodes
                        .into_iter()
                        .map(|(i, w)| w * f64::from(heat.temperature[i]))
                        .sum::<f64>()) as f32,
                    DVec3::ZERO,
                )
            })?);
            let spots = thermal_hot_spots(&case.grid, &heat);
            limits.push("Steady conduction. No radiation or transient heat flow.".into());
            let mut value = json!({"kind":"thermal","min_c":heat.min_c,"max_c":heat.max_c,"hot_spots":spots,
                "iterations":heat.iterations,"mass_g":mass,"method":"Steady trilinear conduction with surface heat transfer."});
            if *stress {
                case.loads.push(Load::Temperature {
                    field: heat.temperature,
                    reference_c: *reference_c,
                    expansion_um_mk: material.expansion_um_mk,
                });
                let result = solve_static_with_progress(&case, |i, v| progress(i, v, None))?;
                fields.push(field(body, &case.grid, &result)?);
                let peak = result.von_mises.iter().copied().fold(0., f32::max);
                if peak > 1e-8 {
                    let r =
                        super::report(id, &material_id, &study, &material, &case.grid, &result)?;
                    value["max_von_mises_mpa"] = json!(r.max_von_mises_mpa);
                    value["max_displacement_mm"] = json!(r.max_displacement_mm);
                    value["min_safety_factor"] = json!(r.min_safety_factor);
                    value["reaction_n"] = json!(r.reaction_n);
                    value["stress_hot_spots"] = json!(r.hot_spots);
                    if let Some(v) = r.layer_safety_factor {
                        value["layer_safety_factor"] = json!(v);
                    }
                    limits.extend(r.limits);
                } else {
                    value["max_von_mises_mpa"] = json!(peak);
                    value["max_displacement_mm"] = json!(
                        result
                            .displacement
                            .iter()
                            .map(|v| DVec3::from(*v).length())
                            .fold(0., f64::max)
                    );
                    value["min_safety_factor"] = Value::Null;
                }
            }
            value
        }
        StudyKind::Shape {
            target_volume,
            keep,
            min_member_mm,
            symmetry,
        } => {
            let mut keep_cells = BTreeSet::new();
            for s in keep {
                keep_cells.extend(faber_sim::cells_near(&case.grid, &face(s)?));
            }
            let placement = faber_core::world_placement(&project.data, &study.part)?;
            let mut planes = Vec::new();
            for name in symmetry {
                let faber_core::Reading::Plane { origin, normal, .. } =
                    project.feature_info(&study.part, name)?.reading
                else {
                    return Err(format!("Feature {name} must be a plane."));
                };
                planes.push(shape::Plane {
                    origin: placement.transform_point3(origin.into()).to_array(),
                    normal: placement.transform_vector3(normal.into()).to_array(),
                });
            }
            let sc = shape::ShapeCase {
                base: case,
                keep: keep_cells.into_iter().collect(),
                target_volume: *target_volume,
                min_member_mm: min_member_mm.unwrap_or(3. * h),
                max_iterations: 80,
                symmetry: planes,
                density_g_cm3: material.density_g_cm3,
            };
            let mut stopped = false;
            let result = shape::optimise_shape_with_poll(&sc, |i, snapshot| {
                let Some((rho, c)) = snapshot else {
                    stopped = !progress(i, 0., None);
                    return !stopped;
                };
                let view = if preview {
                    let mesh = faber_sim::surface::iso_surface(&sc.base.grid, rho, 0.5);
                    (!mesh.triangles.is_empty())
                        .then(|| shape_field(body, &sc.base.grid, rho, mesh))
                } else {
                    None
                };
                stopped = !progress(i, c, view.as_ref());
                !stopped
            })?;
            if stopped {
                return Err("Study stopped.".into());
            }
            let change = match (result.compliance.first(), result.compliance.last()) {
                (Some(&first), Some(&last)) if first > 0. => (last - first) / first,
                _ => 0.,
            };
            fields.push(shape_field(
                body,
                &sc.base.grid,
                &result.density,
                result.mesh.clone(),
            ));
            let mesh_volume = faber_geometry::signed_volume(&result.mesh).abs();
            let mesh_mass = mesh_volume * material.density_g_cm3 / 1000.;
            if !mesh_volume.is_finite() || !mesh_mass.is_finite() {
                return Err("The result mesh volume or mass exceeds the numerical range.".into());
            }
            shape_mesh = Some(Arc::new(result.mesh));
            limits.extend(result.limits);
            for sentence in [
                "The result is a mesh. Model the final part over it or convert it to a solid.",
                "Check the final part with a static study.",
            ] {
                if !limits.iter().any(|s| s == sentence) {
                    limits.push(sentence.into());
                }
            }
            json!({"kind":"shape","method":"Density optimisation with a sensitivity filter.","mass_g":result.mass_g,
                "mass_saved_g":mass-result.mass_g,"mesh_volume_mm3":mesh_volume,"mesh_mass_g":mesh_mass,"volume_share":result.volume_share,"pieces":result.pieces,
                "compliance_change":change,"iterations":result.iterations,"new_part":null})
        }
    };
    report_value["study"] = json!(id);
    report_value["part"] = json!(study.part);
    report_value["material"] = json!(material_id);
    report_value["resolution_mm"] = json!(h);
    report_value["cells"] = json!(cells);
    report_value["limits"] = json!(limits);
    Ok(Analysis {
        report: report_value,
        fields,
        shape_mesh,
    })
}

fn thermal_hot_spots(grid: &Grid, heat: &thermal::ThermalResult) -> Vec<Value> {
    let mut spots = Vec::<(f32, DVec3)>::new();
    for (id, _) in grid.fill.iter().enumerate().filter(|(_, f)| **f >= 0.02) {
        let cell = coordinates(grid, id);
        for corner in 0..8 {
            let [x, y, z] = std::array::from_fn(|axis| cell[axis] + ((corner >> axis) & 1));
            let value = heat.temperature[grid.node(x, y, z)];
            if f64::from(value) < heat.max_c - 1e-4 * (1. + heat.max_c.abs()) {
                continue;
            }
            let point =
                DVec3::from(grid.origin) + DVec3::from([x, y, z].map(|v| v as f64)) * grid.h;
            if spots.iter().all(|(_, p)| p.distance(point) >= 3. * grid.h) {
                spots.push((value, point));
            }
            if spots.len() == 5 {
                break;
            }
        }
        if spots.len() == 5 {
            break;
        }
    }
    spots
        .iter()
        .map(|(t, p)| json!({"point":p.to_array(),"celsius":t}))
        .collect()
}

fn shape_field(body: &Body, grid: &Grid, density: &[f32], mesh: Mesh) -> StudyField {
    let values = mesh
        .vertices
        .iter()
        .map(|p| {
            let q = std::array::from_fn::<_, 3, _>(|a| {
                ((p[a] - grid.origin[a]) / grid.h)
                    .floor()
                    .clamp(0., (grid.n[a] - 1) as f64) as usize
            });
            density[q[0] + grid.n[0] * (q[1] + grid.n[1] * q[2])]
        })
        .collect();
    let zeros = vec![[0.; 3]; mesh.vertices.len()];
    StudyField {
        vertex_von_mises: vec![0.; body.mesh.vertices.len()],
        vertex_displacement: vec![[0.; 3]; body.mesh.vertices.len()],
        surface: Some(Arc::new(StudySurface {
            mesh: Arc::new(mesh),
            face_of_triangle: Arc::new(vec![]),
            vertex_von_mises: values,
            vertex_displacement: zeros,
        })),
    }
}

/// Check the source inputs and the destination before an expensive calculation.
pub fn check_shape_destination(project: &Project, part: &str) -> Result<String> {
    if let Some(source) = project.source() {
        let root = project
            .folder()
            .ok_or("Open the project to save a shape result.")?;
        if project.source_error().is_some()
            || !root.join(faber_build::SOURCE_FILE).is_file()
            || faber_build::needs_build(root)
        {
            return Err(format!("Build {source} before saving the shape result."));
        }
    }
    project.shape_result_id(part)
}

pub fn save_shape(
    project: &mut Project,
    part: &str,
    mesh: &Mesh,
    expected_revision: u64,
) -> Result<String> {
    project.check_edit_revision(expected_revision)?;
    check_shape_destination(project, part)?;
    if mesh.triangles.is_empty() {
        return Err("The result has no surface to save.".into());
    }
    let mut local = mesh.clone();
    let inverse = faber_core::world_placement(&project.data, part)?.inverse();
    for p in &mut local.vertices {
        *p = inverse.transform_point3((*p).into()).to_array();
    }
    local.normals.clear();
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join("shape.off");
    faber_files::write_indexed_mesh(
        &local,
        std::fs::File::create(&path).map_err(|e| e.to_string())?,
    )?;
    let key = project.asset_key(&path)?;
    check_shape_destination(project, part)?;
    project.add_shape_result(expected_revision, part, &path, &key)
}
