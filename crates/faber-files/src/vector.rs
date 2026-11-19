//! Sheet output in millimetres.

mod dxf;
mod paper;
mod pdf;
mod png;
mod svg;

use crate::atomic_write;
use faber_ports::Result;
use faber_ports::vector::{Path2, VectorDoc, VectorExport};
use std::collections::BTreeSet;
use std::path::Path;

pub struct VectorWriter;

impl VectorExport for VectorWriter {
    fn write(&self, doc: &VectorDoc, path: &Path) -> Result<()> {
        self.write_with_width(doc, path, None)
    }
}

impl VectorWriter {
    /// Write closed cut contours for a sheet cutting machine.
    pub fn write_flat_pattern(&self, doc: &VectorDoc, path: &Path) -> Result<()> {
        validate(doc)?;
        let extension = path.extension().and_then(|x| x.to_str()).unwrap_or("");
        let text = match extension.to_ascii_lowercase().as_str() {
            "svg" => svg::render(doc)?,
            "dxf" => dxf::render(doc, true)?,
            _ => return Err("Use .svg or .dxf for a flat pattern.".into()),
        };
        atomic_write(path, text.as_bytes())
    }

    /// Writes the sheet. The optional pixel width applies to a raster preview.
    pub fn write_with_width(
        &self,
        doc: &VectorDoc,
        path: &Path,
        width_px: Option<u32>,
    ) -> Result<()> {
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        validate(doc)?;
        let bytes = match extension.to_ascii_lowercase().as_str() {
            "svg" => svg::render(doc)?.into_bytes(),
            "dxf" => dxf::render(doc, false)?.into_bytes(),
            "pdf" => pdf::render(doc)?,
            "png" => png::render(doc, width_px.unwrap_or(1600))?,
            _ => return Err("Use .svg, .pdf, .dxf or .png for a drawing.".into()),
        };
        atomic_write(path, &bytes)
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.
}

fn point(p: &[f64; 2]) -> bool {
    p.iter().all(|v| v.is_finite())
}

fn single_line(s: &str) -> bool {
    !s.chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{fffe}' | '\u{ffff}'))
}

fn validate(doc: &VectorDoc) -> Result<()> {
    if !doc.size_mm.iter().copied().all(positive) {
        return Err("The sheet width and height must be positive and finite.".into());
    }
    let mut names = BTreeSet::new();
    for layer in &doc.layers {
        if layer.name.trim().is_empty() || !single_line(&layer.name) || !names.insert(&layer.name) {
            return Err(format!(
                "Each layer needs a different single-line name: {}.",
                layer.name
            ));
        }
        if !positive(layer.width_mm) {
            return Err(format!(
                "Layer {} needs a positive finite width.",
                layer.name
            ));
        }
        for path in &layer.paths {
            let valid = match path {
                Path2::Polyline { points, closed } => {
                    points.len() >= if *closed { 3 } else { 2 } && points.iter().all(point)
                }
                Path2::Circle { centre, radius } => point(centre) && positive(*radius),
                Path2::Arc {
                    centre,
                    radius,
                    start_deg,
                    end_deg,
                } => {
                    let span =
                        (end_deg.rem_euclid(360.) - start_deg.rem_euclid(360.)).rem_euclid(360.);
                    point(centre) && positive(*radius) && span.is_finite() && span > 0.
                }
            };
            if !valid {
                return Err(format!("Invalid path on layer {}.", layer.name));
            }
        }
    }
    for label in &doc.texts {
        if !names.contains(&label.layer) {
            return Err(format!("Unknown text layer {}.", label.layer));
        }
        if !point(&label.at) || !positive(label.height_mm) || !single_line(&label.text) {
            return Err(format!("Invalid text on layer {}.", label.layer));
        }
    }
    Ok(())
}
