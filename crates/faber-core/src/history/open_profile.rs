//! A solved open sketch retains its analytic chains and workplane.
use faber_model::{CurvePath, CurveSegment, Placement, SketchLoop, SketchProfile, SketchSegment};
use faber_ports::{Kernel, Result, Shape};
use glam::DVec3;

pub(super) struct OpenProfile {
    pub reason: String,
    pub paths: Vec<SketchLoop>,
    pub plane: Placement,
}
impl OpenProfile {
    pub fn wires(&self, kernel: &dyn Kernel) -> Result<Vec<Shape>> {
        let frame = crate::components::affine(&self.plane);
        let world = |[x, y]: [f64; 2]| frame.transform_point3(DVec3::new(x, y, 0.)).to_array();
        let mut wires = Vec::new();
        for path in &self.paths {
            match path {
                SketchLoop::Path { start, segments } => {
                    let segments = segments
                        .iter()
                        .map(|s| match s {
                            SketchSegment::Line { end } => {
                                Ok(CurveSegment::Line { end: world(*end) })
                            }
                            SketchSegment::Arc { via, end } => Ok(CurveSegment::Arc {
                                via: world(*via),
                                end: world(*end),
                            }),
                            _ => Err("A solved sketch path must have lines and circular arcs."
                                .to_owned()),
                        })
                        .collect::<Result<Vec<_>>>()?;
                    wires.push(kernel.curve(&CurvePath {
                        start: world(*start),
                        segments,
                    })?);
                }
                closed => {
                    let face = kernel.sketch(
                        &SketchProfile {
                            outer: closed.clone(),
                            holes: vec![],
                        },
                        &self.plane,
                    )?;
                    wires.extend(kernel.profile_wires(&face)?);
                }
            }
        }
        if wires.is_empty() {
            return Err("The sketch has no profile edges. Add a line, arc or circle.".into());
        }
        Ok(wires)
    }
}
