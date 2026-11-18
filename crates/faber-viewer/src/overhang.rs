//! Overhang tint. Faces that need support get a colour by severity. Bed faces get blue.

use eframe::egui;
use egui::RichText;
use faber_core::overhang::{self, Report};
use faber_ports::Body;
use std::collections::BTreeMap;

/// Build direction. Z is up.
const UP: [f64; 3] = [0., 0., 1.];
const MUTED: egui::Color32 = egui::Color32::from_rgb(132, 148, 166);

/// Viewer state for the overhang tint.
pub struct State {
    pub on: bool,
    pub critical_deg: f64,
    /// Totals from the last paint. Absent when the tint is off or the analysis fails.
    summary: Option<Report>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on: false,
            critical_deg: 45.,
            summary: None,
        }
    }
}

impl State {
    /// Returns one sRGB colour per triangle for each body. An empty map means no tint.
    pub fn paint(&mut self, bodies: &[Body]) -> BTreeMap<String, Vec<[u8; 3]>> {
        if !self.on {
            self.summary = None;
            return BTreeMap::new();
        }
        let meshes: Vec<&faber_ports::Mesh> = bodies.iter().map(|body| &body.mesh).collect();
        self.summary = overhang::analyse(&meshes, UP, self.critical_deg).ok();
        bodies
            .iter()
            .filter_map(|body| {
                let facings = overhang::classify(&body.mesh, UP, self.critical_deg).ok()?;
                Some((body.id.clone(), overhang::paint(&facings, body.colour)))
            })
            .collect()
    }
}

/// Draws the toggle, the critical angle and the totals. Returns true when the tint must be rebuilt.
pub fn panel(ui: &mut egui::Ui, state: &mut State) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("OVERHANG").size(11.).color(MUTED));
        changed |= ui
            .checkbox(&mut state.on, "")
            .on_hover_text("Colour faces that need support")
            .changed();
    });
    if !state.on {
        return changed;
    }
    ui.add_space(8.);
    changed |= ui
        .add(
            egui::Slider::new(&mut state.critical_deg, 0.0..=90.0)
                .text("Critical")
                .suffix(" deg"),
        )
        .changed();
    ui.label(
        RichText::new("Red: flat. Orange: near critical. Blue: bed.")
            .small()
            .color(MUTED),
    );
    if let Some(report) = &state.summary {
        ui.label(format!(
            "Support {:.1} mm2 of {:.1} mm2",
            report.support_area_mm2, report.total_area_mm2
        ));
        ui.label(format!("Bed {:.1} mm2", report.bed_area_mm2));
        match report.flattest_overhang_deg {
            Some(angle) => ui.label(format!("Flattest overhang {angle:.1} deg")),
            None => ui.label("No support needed"),
        };
        ui.label(format!("Support regions {}", report.regions_total));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use faber_ports::Mesh;

    /// A flat triangle at z = 0. Its winding points down, so it is a bed face.
    fn flat_body() -> Body {
        Body {
            id: "flat".into(),
            name: "Flat".into(),
            group: String::new(),
            colour: [120, 140, 160],
            mesh: Mesh {
                vertices: vec![[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]],
                triangles: vec![[0, 2, 1]],
                normals: Vec::new(),
            },
            volume: 0.,
            bounds: [[0., 0., 0.], [10., 10., 0.]],
            face_of_triangle: Vec::new(),
            edge_lines: Default::default(),
            shape: None,
        }
    }

    #[test]
    fn tint_is_empty_when_off_and_colours_the_bed_when_on() {
        let bodies = [flat_body()];
        let mut state = State::default();
        assert!(state.paint(&bodies).is_empty());
        state.on = true;
        let paint = state.paint(&bodies);
        assert_eq!(paint["flat"], vec![[30, 110, 230]]);
        assert_eq!(state.summary.as_ref().unwrap().bed_area_mm2, 50.);
    }
}
