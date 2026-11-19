//! Prepare a picture from a project snapshot.
use crate::studio;
use faber_core::Project;
use faber_model::{motion::Pose, view::Camera};
use faber_ports::{Body, Mesh, Result};
use faber_scene::{Section, View};
use glam::{DMat4, DQuat, DVec3, EulerRot};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug)]
pub enum Cut {
    Saved(String),
    Plane(Section),
}

#[derive(Clone, Default)]
pub struct Picture {
    pub view: Option<String>,
    pub preset: View,
    pub part: Option<String>,
    pub cut: Option<Cut>,
    pub explode: Option<f64>,
    pub joints: Pose,
    pub style: Option<studio::Style>,
}
pub struct Prepared {
    pub bodies: Vec<Body>,
    pub camera: Camera,
    pub style: studio::Style,
    pub warnings: Vec<String>,
    auto_fit: bool,
}
impl Prepared {
    /// Fit both axes of a portrait picture. Keep the framing of a saved camera.
    pub fn camera_at_size(&self, width: u32, height: u32) -> Camera {
        let mut camera = self.camera;
        if self.auto_fit && width > 0 && width < height {
            let target = DVec3::from(camera.target);
            let scale = faber_scene::Camera::fit_image_scale(width, height);
            camera.eye = (target + (DVec3::from(camera.eye) - target) * scale).to_array();
        }
        camera
    }
}
pub use faber_core::view_pose as pose;
fn merge_cut(mut mesh: Mesh, cap: Mesh) -> Mesh {
    let start = mesh.vertices.len() as u32;
    if !mesh.normals.is_empty() && cap.normals.len() == cap.vertices.len() {
        mesh.normals.extend(cap.normals);
    } else if !cap.vertices.is_empty() {
        mesh.normals.clear();
    }
    mesh.vertices.extend(cap.vertices);
    mesh.triangles
        .extend(cap.triangles.into_iter().map(|t| t.map(|i| i + start)));
    mesh
}
fn bounds(mesh: &Mesh) -> [[f64; 3]; 2] {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = -lo;
    for &p in &mesh.vertices {
        lo = lo.min(p.into());
        hi = hi.max(p.into());
    }
    [lo.to_array(), hi.to_array()]
}
fn transform(body: &mut Body, matrix: DMat4) {
    if matrix == DMat4::IDENTITY {
        return;
    }
    for p in body
        .mesh
        .vertices
        .iter_mut()
        .chain(&mut body.edge_lines.points)
    {
        *p = matrix.transform_point3((*p).into()).to_array();
    }
    for n in &mut body.mesh.normals {
        *n = matrix.transform_vector3((*n).into()).to_array();
    }
    body.bounds = bounds(&body.mesh);
    body.shape = None;
}
pub fn prepare(project: &Project, options: &Picture) -> Result<Prepared> {
    let name = options.view.as_deref();
    let saved = name
        .map(|id| {
            project
                .data
                .views
                .get(id)
                .ok_or_else(|| format!("Unknown view: {id}"))
        })
        .transpose()?;
    let mut warnings = Vec::new();
    let hidden: BTreeSet<_> = saved.into_iter().flat_map(|v| &v.hidden).collect();
    for id in &hidden {
        if !project.data.parts.contains_key(*id) {
            warnings.push(format!(
                "View {}: part {id} was not found.",
                name.unwrap_or_default()
            ));
        }
    }
    let mut joints = name
        .map(|key| pose(&project.data, key))
        .transpose()?
        .unwrap_or_default();
    joints.extend(options.joints.clone());
    let placements = if joints.is_empty() {
        BTreeMap::new()
    } else {
        project.data.motion.solve_pose(&joints)?
    };
    // Keep the same assembly directions as the interactive explode control.
    let factor = options
        .explode
        .or_else(|| saved.and_then(|v| v.explode))
        .unwrap_or(0.);
    let shifts: BTreeMap<_, _> =
        faber_core::explode::offsets(&project.bodies, &project.data.motion, factor)?
            .into_iter()
            .map(|o| (o.part, o.vector))
            .collect();
    let mut bodies = project.bodies.clone();
    for body in &mut bodies {
        if let Some(p) = placements.get(&body.id) {
            let [x, y, z] = p.rotation.map(f64::to_radians);
            transform(
                body,
                DMat4::from_rotation_translation(
                    DQuat::from_euler(EulerRot::ZYX, z, y, x),
                    p.translation.into(),
                ),
            );
        }
    }
    for body in &mut bodies {
        if let Some(v) = shifts.get(&body.id) {
            transform(body, DMat4::from_translation((*v).into()));
        }
    }
    if let Some(id) = &options.part
        && !project.data.parts.contains_key(id)
    {
        return Err(format!("Unknown render part: {id}"));
    }
    bodies.retain(|b| !hidden.contains(&b.id) && options.part.as_ref().is_none_or(|p| *p == b.id));
    let mut fit = faber_scene::Camera::fit(&bodies);
    fit.set_view(options.preset);
    let camera = saved.map_or_else(|| fit.to_named(), |v| v.camera);
    camera.validate()?;
    let cut = options.cut.clone().or_else(|| {
        let id = saved?.section.as_ref()?;
        if project.data.sections.contains_key(id) {
            Some(Cut::Saved(id.clone()))
        } else {
            warnings.push(format!(
                "View {}: section {id} was not found.",
                name.unwrap_or_default()
            ));
            None
        }
    });
    if let Some(cut) = cut {
        let (plane, scope) = match cut {
            Cut::Plane(p) => (p, vec![]),
            Cut::Saved(id) => {
                let s = project.section(&id)?;
                (
                    Section {
                        normal: s.normal,
                        offset: DVec3::from(s.normal).dot(s.origin.into()),
                        reverse: s.reverse,
                    },
                    s.parts,
                )
            }
        };
        let mut drawn = Vec::new();
        for mut body in bodies {
            if !scope.is_empty() && !scope.contains(&body.id) {
                drawn.push(body);
                continue;
            }
            let cut = faber_scene::section(&body.mesh, plane)?;
            body.mesh = merge_cut(cut.surface, cut.cap);
            if body.mesh.triangles.is_empty() {
                continue;
            }
            body.bounds = bounds(&body.mesh);
            body.shape = None;
            body.edge_lines = Default::default();
            body.face_of_triangle.clear();
            drawn.push(body);
        }
        bodies = drawn;
    }
    let style = options.style.unwrap_or_else(|| {
        if saved.and_then(|v| v.style.as_deref()) == Some("studio") {
            studio::Style::Studio
        } else {
            studio::Style::Technical
        }
    });
    Ok(Prepared {
        auto_fit: saved.is_none(),
        bodies,
        camera,
        style,
        warnings,
    })
}

pub fn check_picture_path(path: &Path) -> Result<()> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err("Use a .png file for the picture.".into());
    }
    Ok(())
}
pub fn save_frame(path: &Path, rgba: Vec<u8>, width: u32, height: u32) -> Result<()> {
    check_picture_path(path)?;
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or("The image has the wrong pixel count.")?;
    let parent = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    image
        .write_to(file.as_file_mut(), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}
