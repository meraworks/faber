//! Adapter composition.

pub mod archive;

pub mod motion_check;

pub use faber_clash::Report as InterferenceReport;

struct MeshChecks;
impl faber_ports::DesignChecks for MeshChecks {
    fn errors(
        &self,
        contacts: &std::collections::BTreeMap<String, Contact>,
        bodies: &[Body],
    ) -> Result<std::collections::BTreeMap<String, String>> {
        let report = interference(bodies, 0.1, contacts)?;
        Ok(report
            .violations
            .into_iter()
            .map(|finding| {
                (
                    format!("{:?}:{}:{}", finding.kind, finding.a, finding.b),
                    format!("{} and {}: {:?}", finding.a, finding.b, finding.kind),
                )
            })
            .collect())
    }
}
use faber_core::Project;
use faber_files::{FileStore, MeshExchange};
use faber_geometry::NativeKernel;
use faber_kernel_occt::OcctKernel;
use faber_model::{Contact, ProjectData};
use faber_ports::{Body, Exchange, Kernel, ProjectStore, Result};
use std::path::Path;
use std::sync::Arc;

pub fn sketch_preview(profile: &faber_model::SketchProfile) -> Result<faber_ports::Mesh> {
    let shape = OcctKernel.sketch(profile, &Default::default())?;
    OcctKernel.tessellate(&shape, 0.05)
}

/// Checks placed bodies. Declared contacts give no violation.
pub fn interference(
    bodies: &[Body],
    proximity_mm: f64,
    contacts: &std::collections::BTreeMap<String, Contact>,
) -> Result<faber_clash::Report> {
    use faber_clash::{Part, PartKind, ScanOptions, Scene};
    use parry3d_f64::{
        math::Vector,
        shape::{TriMesh, TriMeshFlags},
    };
    if !proximity_mm.is_finite() || proximity_mm < 0. {
        return Err("Proximity must be finite and non-negative".into());
    }
    let parts = bodies
        .iter()
        .map(|body| {
            let vertices = body
                .mesh
                .vertices
                .iter()
                .map(|p| Vector::new(p[0], p[1], p[2]))
                .collect();
            let mesh = TriMesh::with_flags(
                vertices,
                body.mesh.triangles.clone(),
                TriMeshFlags::ORIENTED | TriMeshFlags::MERGE_DUPLICATE_VERTICES,
            )
            .map_err(|e| format!("{}: {e}", body.id))?;
            Ok(Part {
                name: body.id.clone(),
                kind: PartKind::Printed,
                movable: false,
                mesh,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let declared: Vec<&Contact> = contacts
        .values()
        .filter(|c| bodies.iter().any(|b| b.id == c.a) && bodies.iter().any(|b| b.id == c.b))
        .collect();
    let scene = Scene {
        signature: "current-design".into(),
        parts,
        poses: vec![],
        rules: faber_clash::scene::Rules {
            allowed_contacts: declared
                .iter()
                .map(|c| faber_clash::scene::Rules::key(&c.a, &c.b))
                .collect(),
            contact_limits_mm: declared
                .iter()
                .map(|c| (faber_clash::scene::Rules::key(&c.a, &c.b), c.max_depth_mm))
                .collect(),
            allowed_interference: Default::default(),
            excluded_pairs: Default::default(),
            proximity_mm,
        },
    };
    let mut report = faber_clash::scan(
        &scene,
        &ScanOptions {
            static_only: true,
            threads: 6,
        },
    )?;
    confirm_contacts(bodies, contacts, &mut report)?;
    confirm_declared(bodies, contacts, &mut report)?;
    Ok(report)
}

/// Common volume at or below this value, in cubic millimetres, is contact.
pub const CONTACT_VOLUME_MM3: f64 = 1e-6;

/// Properties of the exact common part of two bodies. Missing shapes give None.
fn exact_common(
    bodies: &[Body],
    a_id: &str,
    b_id: &str,
) -> Result<Option<faber_ports::Properties>> {
    use faber_ports::Kernel;
    let shape = |id: &str| {
        bodies
            .iter()
            .find(|body| body.id == id)
            .and_then(|body| body.shape.as_ref())
    };
    let (Some(a), Some(b)) = (shape(a_id), shape(b_id)) else {
        return Ok(None);
    };
    // An empty common part has no volume and no bounds.
    let properties = match OcctKernel.intersect_solids(a, b)? {
        Some(common) => OcctKernel.properties(&common)?,
        None => faber_ports::Properties {
            volume: 0.,
            centre: [0.; 3],
            bounds: [[0.; 3]; 2],
            faces: 0,
            edges: 0,
            solids: 0,
            valid: true,
        },
    };
    Ok(Some(properties))
}

/// Smallest bounding extent of the common part, in millimetres.
fn thickness_mm(properties: &faber_ports::Properties) -> f64 {
    let [min, max] = properties.bounds;
    (0..3)
        .map(|i| max[i] - min[i])
        .fold(f64::INFINITY, f64::min)
}

fn set_status(report: &mut faber_clash::Report) {
    report.status = if report.violations.is_empty() {
        faber_clash::Status::Pass
    } else {
        faber_clash::Status::Fail
    };
}

/// Each pair that the mesh check reports as contact must have no common volume in the exact kernel.
/// A pair with volume is interference. The mesh depth can be too small.
fn confirm_contacts(
    bodies: &[Body],
    contacts: &std::collections::BTreeMap<String, Contact>,
    report: &mut faber_clash::Report,
) -> Result<()> {
    for mut finding in std::mem::take(&mut report.contacts) {
        let Some(properties) = exact_common(bodies, &finding.a, &finding.b)? else {
            finding.kind = faber_clash::FindingKind::Interference;
            report.violations.push(finding);
            continue;
        };
        if properties.volume <= CONTACT_VOLUME_MM3 {
            // A declared pair that only touches is an allowed contact.
            if contacts.contains_key(&faber_core::contact_key(&finding.a, &finding.b)) {
                finding.kind = faber_clash::FindingKind::AllowedContact;
                report.allowed_contacts.push(finding);
            } else {
                report.contacts.push(finding);
            }
        } else {
            finding.kind = faber_clash::FindingKind::Interference;
            finding.depth_mm = finding.depth_mm.max(thickness_mm(&properties));
            report.violations.push(finding);
        }
    }
    set_status(report);
    Ok(())
}

/// A declared pair that the mesh check finds deeper than its limit is confirmed exactly.
/// No common volume means touch, so the pair is allowed. Otherwise it stays a violation.
fn confirm_declared(
    bodies: &[Body],
    contacts: &std::collections::BTreeMap<String, Contact>,
    report: &mut faber_clash::Report,
) -> Result<()> {
    let mut kept = Vec::new();
    for mut finding in std::mem::take(&mut report.violations) {
        let limit = contacts
            .get(&faber_core::contact_key(&finding.a, &finding.b))
            .map(|c| c.max_depth_mm);
        let over_limit = finding.kind == faber_clash::FindingKind::Interference
            && limit.is_some_and(|limit| finding.depth_mm > limit);
        let Some(properties) = over_limit
            .then(|| exact_common(bodies, &finding.a, &finding.b))
            .transpose()?
            .flatten()
        else {
            kept.push(finding);
            continue;
        };
        if properties.volume <= CONTACT_VOLUME_MM3 {
            finding.kind = faber_clash::FindingKind::AllowedContact;
            report.allowed_contacts.push(finding);
        } else {
            finding.depth_mm = finding.depth_mm.max(thickness_mm(&properties));
            kept.push(finding);
        }
    }
    report.violations = kept;
    set_status(report);
    Ok(())
}

/// The design source of a project folder. It checks the parts with the OCCT kernel.
pub fn design_source() -> Arc<dyn faber_ports::source::DesignSource> {
    Arc::new(faber_build::JsonnetSource::new(Arc::new(OcctKernel)))
}

/// Reads the saved project. A generated project builds first when an input
/// changed. A failed build does not fail the call. It sets the source error,
/// and the project keeps its last good state.
pub fn refresh(path: &Path, project: &mut Project) -> Result<()> {
    project.sync()?;
    let source = design_source();
    project.set_source(
        source
            .has_source(path)
            .then(|| source.file_name().to_string()),
    );
    project.set_source_error(None);
    if project.source().is_some()
        && faber_build::needs_build(path)
        && let Err(error) = faber_build::build_project(path, project)
    {
        project.set_source_error(Some(error));
    }
    Ok(())
}

/// Builds the design source now. The project then reads the new files.
pub fn build_source(path: &Path, project: &mut Project) -> Result<faber_ports::source::Report> {
    let report = faber_build::build_project(path, project)?;
    project.set_source(Some(design_source().file_name().to_string()));
    Ok(report)
}

/// Moves the history. A generated project gets the recipes of its source again.
/// The build record is removed first. A failure then leaves the next call to build.
pub fn history_move(
    path: &Path,
    project: &mut Project,
    action: faber_ports::HistoryMove,
) -> Result<()> {
    if project.source().is_none() {
        return match action {
            faber_ports::HistoryMove::Undo => project.undo(),
            faber_ports::HistoryMove::Redo => project.redo(),
        };
    }
    faber_build::forget_build(path)?;
    project.move_history_with(action, &|data| regenerated(path, data))?;
    faber_build::record_build(path, &project.data)
}

/// Restores a checkpoint. A generated project gets the recipes of its source again.
/// The build record is removed first, as for `history_move`.
pub fn restore(path: &Path, project: &mut Project, name: &str) -> Result<()> {
    if project.source().is_none() {
        return project.restore(name);
    }
    faber_build::forget_build(path)?;
    project.restore_with(name, &|data| regenerated(path, data))?;
    faber_build::record_build(path, &project.data)
}

fn regenerated(path: &Path, data: ProjectData) -> Result<ProjectData> {
    faber_build::evaluate(path, &data)
        .map(|candidate| candidate.data)
        .map_err(|error| format!("The design source has an error: {error}"))
}

pub fn open(path: &Path, create: bool, name: Option<String>) -> Result<Project> {
    let source = design_source();
    let generated = source.has_source(path);
    if !create && !generated && !path.join("faber.json").exists() {
        return Err("Project does not exist".into());
    }
    let store = Arc::new(FileStore::open(path)?);
    if (create || generated) && !path.join("faber.json").exists() {
        let data = ProjectData {
            schema: 1,
            revision: 0,
            name: name.unwrap_or_else(|| "Untitled".into()),
            parts: Default::default(),
            params: Default::default(),
            contacts: Default::default(),
            motion: Default::default(),
            sections: Default::default(),
        };
        faber_core::build(&data, &NativeKernel)?;
        store.save(None, &data)?;
    }
    let mut project = Project::open_with_cache(
        store,
        Arc::new(OcctKernel),
        Some(Arc::new(faber_files::FileAssets::new(path))),
        Some(Arc::new(faber_files::FileFeatureCache::new(path))),
    )?
    .with_checks(Arc::new(MeshChecks));
    project.set_source(generated.then(|| source.file_name().to_string()));
    if generated {
        refresh(path, &mut project)?;
    }
    Ok(project)
}

pub fn read(path: &Path) -> Result<(ProjectData, Vec<Body>)> {
    let data = faber_files::read(path)?;
    let bodies = faber_core::build_with_cache(
        &data,
        &OcctKernel,
        Some(&faber_files::FileAssets::new(path)),
        Some(&faber_files::FileFeatureCache::new(path)),
    )?;
    Ok((data, bodies))
}

/// A value that changes when a project file changes. The viewer watches it.
pub fn project_fingerprint(path: &Path) -> Result<(std::time::SystemTime, u64)> {
    faber_files::fingerprint(path)
}

pub fn create(path: &Path, name: String) -> Result<Project> {
    let store = Arc::new(FileStore::open(path)?);
    if path.join("faber.json").exists() {
        return Err("A project already exists in this folder".into());
    }
    let data = ProjectData {
        schema: 1,
        revision: 0,
        name,
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    };
    faber_core::build(&data, &NativeKernel)?;
    store.save(None, &data)?;
    Ok(Project::open_with_cache(
        store,
        Arc::new(OcctKernel),
        Some(Arc::new(faber_files::FileAssets::new(path))),
        Some(Arc::new(faber_files::FileFeatureCache::new(path))),
    )?
    .with_checks(Arc::new(MeshChecks)))
}

/// Default tessellation tolerance of an STL export by the MCP tool and the CLI, in millimetres.
pub const EXPORT_TOLERANCE_MM: f64 = 0.01;

/// Tessellation tolerance of the library exports, in millimetres.
const LIBRARY_TOLERANCE_MM: f64 = 0.001;

/// One written file. Triangles are counted for an STL file only.
pub struct ExportFile {
    pub path: String,
    pub triangles: Option<usize>,
}

impl ExportFile {
    /// The file as a reply entry. The triangle count is absent for a STEP file.
    pub fn to_json(&self) -> serde_json::Value {
        match self.triangles {
            Some(triangles) => serde_json::json!({"path":self.path,"triangles":triangles}),
            None => serde_json::json!({"path":self.path}),
        }
    }
}

pub fn export(bodies: &[Body], path: &Path) -> Result<Vec<String>> {
    Ok(export_meshes(bodies, path, LIBRARY_TOLERANCE_MM)?
        .into_iter()
        .map(|file| file.path)
        .collect())
}

fn export_meshes(bodies: &[Body], path: &Path, tolerance: f64) -> Result<Vec<ExportFile>> {
    let mut ids = std::collections::BTreeSet::new();
    for body in bodies {
        if !faber_core::valid_id(&body.id) || !ids.insert(body.id.to_ascii_lowercase()) {
            return Err(format!("Invalid or duplicate output name: {}", body.id));
        }
    }
    let mut files = Vec::new();
    for body in bodies {
        let output = path.join(format!("{}.stl", body.id));
        let print_mesh;
        let mesh = match &body.shape {
            Some(shape) => {
                print_mesh = OcctKernel.tessellate(shape, tolerance)?;
                &print_mesh
            }
            None => &body.mesh,
        };
        MeshExchange.write(mesh, &output)?;
        files.push(ExportFile {
            path: output.to_string_lossy().into_owned(),
            triangles: Some(mesh.triangles.len()),
        });
    }
    Ok(files)
}

/// Lists each refused finding with its parts and its measure.
fn refusal(
    findings: &[faber_clash::Finding],
    contacts: &std::collections::BTreeMap<String, Contact>,
) -> String {
    let count = findings.len();
    let noun = if count == 1 { "finding" } else { "findings" };
    let mut text = format!("Export refused: {count} interference or clearance {noun}.");
    let mut over_limit = false;
    for finding in findings {
        let limit = contacts
            .get(&faber_core::contact_key(&finding.a, &finding.b))
            .map(|c| c.max_depth_mm)
            .filter(|limit| finding.depth_mm > *limit);
        if let Some(limit) = limit {
            over_limit = true;
            text.push_str(&format!(
                "\n{} and {}: interference, depth {:.3} mm, above the declared contact limit of {limit:.3} mm.",
                finding.a, finding.b, finding.depth_mm
            ));
            continue;
        }
        let (kind, measure) = match finding.kind {
            faber_clash::FindingKind::Clearance => {
                ("clearance", format!("gap {:.3} mm", finding.gap_mm))
            }
            _ => ("interference", format!("depth {:.3} mm", finding.depth_mm)),
        };
        text.push_str(&format!(
            "\n{} and {}: {kind}, {measure}.",
            finding.a, finding.b
        ));
    }
    if over_limit {
        text.push_str(
            "\nReduce the overlap, or raise max_depth_mm with set_contact, or use force: true.",
        );
    } else {
        text.push_str("\nDeclare an intended contact with set_contact, or use force: true.");
    }
    text
}

pub fn export_checked(
    bodies: &[Body],
    contacts: &std::collections::BTreeMap<String, Contact>,
    path: &Path,
    format: &str,
    force: bool,
) -> Result<Vec<String>> {
    Ok(
        export_checked_with(bodies, contacts, path, format, force, LIBRARY_TOLERANCE_MM)?
            .into_iter()
            .map(|file| file.path)
            .collect(),
    )
}

/// Writes one file per body after the interference check. The tolerance applies to STL only.
pub fn export_checked_with(
    bodies: &[Body],
    contacts: &std::collections::BTreeMap<String, Contact>,
    path: &Path,
    format: &str,
    force: bool,
    tolerance: f64,
) -> Result<Vec<ExportFile>> {
    if !matches!(format, "stl" | "step") {
        return Err("Format must be stl or step".into());
    }
    if !(0.001..=1.0).contains(&tolerance) {
        return Err(format!(
            "Tolerance {tolerance} mm is outside 0.001 to 1.0 mm."
        ));
    }
    if !force {
        let report = interference(bodies, 0.1, contacts)?;
        if !report.violations.is_empty() {
            return Err(refusal(&report.violations, contacts));
        }
    }
    match format {
        "stl" => export_meshes(bodies, path, tolerance),
        _ => Ok(export_exact(bodies, path)?
            .into_iter()
            .map(|path| ExportFile {
                path,
                triangles: None,
            })
            .collect()),
    }
}

pub fn export_exact(bodies: &[Body], path: &Path) -> Result<Vec<String>> {
    let mut ids = std::collections::BTreeSet::new();
    for body in bodies {
        if !faber_core::valid_id(&body.id) || !ids.insert(body.id.to_ascii_lowercase()) {
            return Err("Invalid or duplicate output name".into());
        }
        if body.shape.is_none() {
            return Err("Body has no exact shape".into());
        }
    }
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for body in bodies {
        let output = path.join(format!("{}.step", body.id));
        OcctKernel.write_shape(
            body.shape.as_ref().ok_or("Body has no exact shape")?,
            &output,
        )?;
        files.push(output.to_string_lossy().into_owned());
    }
    Ok(files)
}

pub fn render(
    bodies: &[Body],
    path: &Path,
    section: Option<faber_scene::Section>,
    width: u32,
    height: u32,
) -> Result<()> {
    render_view(bodies, path, section, width, height, faber_scene::View::Iso)
}

pub fn render_view(
    bodies: &[Body],
    path: &Path,
    section: Option<faber_scene::Section>,
    width: u32,
    height: u32,
    view: faber_scene::View,
) -> Result<()> {
    if !(64..=4096).contains(&width) || !(64..=4096).contains(&height) {
        return Err("Image size must be between 64 and 4096 pixels".into());
    }
    faber_render::save_view(bodies, path, section, width, height, view)
}
