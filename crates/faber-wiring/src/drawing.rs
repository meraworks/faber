//! Associative sheet export from the placed project geometry.
mod anchors;
mod project_view;

use faber_core::Project;
pub use faber_drawing::Rendered;
use faber_drawing::{ModelSource, PartRow, Resolved};
use faber_model::drawing::{Anchor, Drawing, Sheet};
use faber_ports::{
    Body, Result,
    drawing::{Projection2, ViewFrame},
};
use std::{collections::BTreeSet, path::Path};

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
                let rows = faber_core::bom::selected(
                    &self.project.data,
                    self.bodies(parts)?.into_iter(),
                    None,
                    true,
                );
                rows.into_iter()
                    .map(|row| {
                        let material = row
                            .material
                            .as_deref()
                            .map(|id| faber_core::materials::get(&self.project.data, id))
                            .transpose()?;
                        Ok(PartRow {
                            quantity: row.quantity as usize,
                            name: row.name,
                            material: material.map(|m| m.name).unwrap_or_default(),
                            mass_g: row.mass_g.map(|m| m / f64::from(row.quantity)),
                        })
                    })
                    .collect()
            })
    }
    fn limits(&self, parts: &[String]) -> Vec<String> {
        self.bodies(parts).unwrap_or_default().into_iter().filter(|b|b.shape.is_none())
            .map(|b|format!("Mesh part {} shows its projected outline only. It does not hide exact edges or supply a section hatch.",b.id)).collect()
    }
}

pub use crate::read_snapshot as open_snapshot;

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
