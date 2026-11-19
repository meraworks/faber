use crate::Project;
use faber_model::{Geometry, Placement, Scalar};
use faber_ports::{Combine, Kernel, Result, Shape};
use faber_sketch::{Sketch, Solution, Status, edit::SketchOp, profile};
use std::collections::BTreeMap;

/// Resolve sketch dimensions with the current saved parameters.
pub fn resolve_sketch(sketch: &Sketch<Scalar>, project: Option<&Project>) -> Result<Sketch> {
    let values = match project {
        Some(project) => project.params()?,
        None => BTreeMap::new(),
    };
    sketch.map_numbers(&mut |value| crate::number(value, &values))
}

pub(crate) fn solve(sketch: &Sketch) -> Result<Solution> {
    let solution = faber_sketch::solve(sketch)?;
    if solution.status == Status::Over {
        return Err(format!(
            "Sketch constraints conflict at indices {:?}.",
            solution.conflicting
        ));
    }
    Ok(solution)
}

fn join_faces(
    kernel: &dyn Kernel,
    faces: impl IntoIterator<Item = Result<Shape>>,
) -> Result<Shape> {
    let mut faces = faces.into_iter();
    let first = faces.next().ok_or("The sketch has no closed profile.")??;
    faces.try_fold(first, |out, face| {
        kernel.combine(&out, &face?, Combine::Union)
    })
}

pub(crate) fn face(
    kernel: &dyn Kernel,
    contours: &[profile::Loop],
    plane: &Placement,
) -> Result<Shape> {
    let regions = profile::regions(contours);
    join_faces(
        kernel,
        regions.iter().map(|region| kernel.sketch(region, plane)),
    )
}

pub(crate) fn offset(kernel: &dyn Kernel, sketch: &Shape, distance: f64) -> Result<Shape> {
    let props = kernel.summary_properties(sketch)?;
    if props.solids != 0 || props.faces == 0 {
        return Err("Select a sketch with planar faces.".into());
    }
    join_faces(
        kernel,
        (1..=props.faces)
            .map(|i| kernel.offset_sketch_face(&kernel.face_shape(sketch, i)?, distance)),
    )
}

impl Project {
    /// Part and feature names with the remaining degrees of freedom.
    pub fn sketch_warnings(&self) -> BTreeMap<String, BTreeMap<String, usize>> {
        self.histories
            .iter()
            .filter_map(|(part, history)| {
                let warnings = history.sketch_warnings();
                (!warnings.is_empty()).then(|| (part.clone(), warnings))
            })
            .collect()
    }

    /// Apply the edits as a single history change. Dimensions retain their parameter expressions.
    pub fn edit_sketch(
        &mut self,
        part: &str,
        feature: &str,
        ops: &[SketchOp<Scalar>],
    ) -> Result<()> {
        let item = self
            .data
            .parts
            .get(part)
            .ok_or_else(|| format!("Unknown part: {part}"))?;
        crate::refuse_link_edit(part, item)?;
        crate::refuse_instance_edit(part, item)?;
        let Geometry::History { features, .. } = &item.geometry else {
            return Err(format!("Part {part} has no feature history."));
        };
        let entry = features
            .iter()
            .find(|f| f.id == feature)
            .ok_or_else(|| format!("Unknown feature: {feature}"))?;
        let Geometry::SolvedSketch { plane, sketch } = &entry.geometry else {
            return Err(format!(
                "Select a constrained sketch. Feature {feature} is {}.",
                crate::history::operation_kind(&entry.geometry)
            ));
        };
        if entry.suppressed {
            return Err(format!("Unsuppress sketch {feature} before editing it."));
        }
        if ops.is_empty() {
            return Err("Supply at least one sketch edit.".into());
        }
        let params = self.params()?;
        let mut value = |s: &Scalar| crate::number(s, &params);
        let sketch = faber_sketch::edit::apply_parametric(sketch, ops, &mut value)?;
        self.edit_feature(
            part,
            feature,
            Geometry::SolvedSketch {
                plane: plane.clone(),
                sketch,
            },
        )
    }
}
