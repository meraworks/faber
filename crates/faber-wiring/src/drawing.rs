//! Associative sheet export from the placed project geometry.
mod anchors;
mod project_view;

use faber_core::Project;
pub use faber_drawing::Rendered;
use faber_drawing::{ModelSource, PartRow, Resolved};
use faber_model::{
    Geometry,
    drawing::{Anchor, Drawing, Sheet},
};
use faber_ports::{
    Body, Result,
    drawing::{Projection2, ViewFrame},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct DrawingModel<'a> {
    project: &'a Project,
}
impl<'a> DrawingModel<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }
    fn body(&self, id: &str) -> Result<&'a Body> {
        self.project
            .bodies
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| format!("Unknown drawing part: {id}."))
    }
    fn bodies(&self, parts: &[String]) -> Result<Vec<&'a Body>> {
        if parts.is_empty() {
            return Ok(self.project.bodies.iter().collect());
        }
        parts
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|id| self.body(id))
            .collect()
    }
}
impl ModelSource for DrawingModel<'_> {
    fn project(&self, parts: &[String], frame: &ViewFrame, hidden: bool) -> Result<Projection2> {
        let key = format!("{parts:?}/{frame:?}/{hidden}");
        self.project.cached("drawing_projection", &key, || {
            project_view::draw(&self.bodies(parts)?, frame, hidden, false)
        })
    }
    fn cut_view(&self, parts: &[String], frame: &ViewFrame, hidden: bool) -> Result<Projection2> {
        let key = format!("{parts:?}/{frame:?}/{hidden}");
        self.project.cached("drawing_cut_view", &key, || {
            project_view::draw(&self.bodies(parts)?, frame, hidden, true)
        })
    }
    fn bounds(&self, parts: &[String]) -> Result<[[f64; 3]; 2]> {
        self.bodies(parts)?
            .iter()
            .map(|b| b.bounds)
            .reduce(faber_core::union_box)
            .ok_or("A drawing needs at least one part.".into())
    }
    fn section_in_frame(&self, parts: &[String], frame: &ViewFrame) -> Result<Vec<Vec<[f64; 2]>>> {
        self.project
            .cached("drawing_section", &format!("{parts:?}/{frame:?}"), || {
                project_view::section(&self.bodies(parts)?, frame)
            })
    }
    fn section(
        &self,
        parts: &[String],
        origin: [f64; 3],
        normal: [f64; 3],
    ) -> Result<Vec<Vec<[f64; 2]>>> {
        let plane = faber_core::pick_selector::plane_at(origin, normal)?;
        let basis = faber_core::rotation_matrix(plane.rotation);
        self.section_in_frame(
            parts,
            &ViewFrame {
                origin,
                x: basis.x_axis.into(),
                y: basis.y_axis.into(),
            },
        )
    }
    fn resolve(&self, anchor: &Anchor) -> Result<Resolved> {
        self.project
            .cached("drawing_anchor", &format!("{anchor:?}"), || {
                anchors::resolve(self, anchor)
            })
    }
    fn anchors(&self, parts: &[String]) -> Result<Vec<(Anchor, Resolved)>> {
        self.project
            .cached("drawing_anchors", &format!("{parts:?}"), || {
                anchors::enumerate(self, &self.bodies(parts)?)
            })
    }
    fn parts_list(&self, parts: &[String]) -> Result<Vec<PartRow>> {
        self.project
            .cached("drawing_parts_list", &format!("{parts:?}"), || {
                let mut rows = BTreeMap::<String, PartRow>::new();
                for body in self.bodies(parts)? {
                    let part = &self.project.data.parts[&body.id];
                    let (geometry, _) = self.project.local_geometry(&body.id)?;
                    let key = serde_json::to_string(&(geometry, &part.material))
                        .map_err(|e| e.to_string())?;
                    if let Some(row) = rows.get_mut(&key) {
                        row.quantity += 1;
                        continue;
                    }
                    let material = part
                        .material
                        .as_ref()
                        .map(|id| faber_core::materials::get(&self.project.data, id))
                        .transpose()?;
                    let purchased_mass = match geometry {
                        Geometry::Imported {
                            purchased: Some(item),
                            ..
                        } => item.mass_g,
                        _ => None,
                    };
                    let mass_g = match (purchased_mass, &material) {
                        (Some(m), _) => Some(m),
                        (_, Some(m)) if body.shape.is_some() => {
                            Some(body.volume * m.density_g_cm3 / 1000.)
                        }
                        (_, Some(m)) => crate::mesh_edit::report(self.project, &body.id)?
                            .volume_mm3
                            .map(|v| v.abs() * m.density_g_cm3 / 1000.),
                        _ => None,
                    };
                    rows.insert(
                        key,
                        PartRow {
                            quantity: 1,
                            name: body.name.clone(),
                            material: material.map(|m| m.name).unwrap_or_default(),
                            mass_g,
                        },
                    );
                }
                Ok(rows.into_values().collect())
            })
    }
    fn limits(&self, parts: &[String]) -> Vec<String> {
        self.bodies(parts).unwrap_or_default().into_iter().filter(|b|b.shape.is_none())
            .map(|b|format!("Mesh part {} shows its projected outline only. It does not hide exact edges or supply a section hatch.",b.id)).collect()
    }
}

/// Read the current design without taking the editor's file lock.
pub fn open_snapshot(path: &Path) -> Result<Project> {
    for _ in 0..3 {
        let before = faber_files::fingerprint(path)?;
        let data = faber_files::read_committed(path);
        if before == faber_files::fingerprint(path)? {
            let cache = faber_files::FileFeatureCache::read_only(path);
            return crate::preview_with_cache(path, data?, Some(std::sync::Arc::new(cache)));
        }
    }
    Err("The design changed while the drawing was loading.".into())
}

pub fn render(project: &Project, id: &str) -> Result<Rendered> {
    let drawing = project
        .data
        .drawings
        .get(id)
        .ok_or_else(|| format!("Unknown drawing: {id}."))?;
    project.cached("render_drawing", id, || {
        faber_drawing::render(drawing, &DrawingModel::new(project))
    })
}
pub fn auto_drawing(
    project: &Project,
    parts: &[String],
    sheet: Sheet,
    title: &str,
) -> Result<Drawing> {
    faber_drawing::auto_drawing(&DrawingModel::new(project), parts, sheet, title)
}
pub fn export(project: &Project, id: &str, path: &Path, width_px: Option<u32>) -> Result<Rendered> {
    let rendered = render(project, id)?;
    write_rendered(&rendered, path, width_px)?;
    Ok(rendered)
}

/// Write the sheet that is visible in the preview.
pub fn write_rendered(rendered: &Rendered, path: &Path, width_px: Option<u32>) -> Result<()> {
    faber_files::VectorWriter.write_with_width(&rendered.doc, path, width_px)
}
