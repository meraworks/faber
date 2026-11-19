//! Read a folded wall tree or export its flat pattern in part coordinates.
use faber_core::Project;
use faber_model::{Flange, Geometry, ProjectData, Scalar, SheetRule, SketchProfile};
use faber_ports::Result;
pub use faber_sheet::{BendLine, Direction, FlatPattern};
use glam::DVec3;
use serde_json::{Value, json};
use std::path::Path;

fn output<N>(geometry: &Geometry<N>) -> Option<&Geometry<N>> {
    match geometry {
        Geometry::SheetMetal { .. } => Some(geometry),
        Geometry::History { features, output } => features
            .iter()
            .find(|f| f.id == *output && !f.suppressed)
            .and_then(|f| match &f.geometry {
                sheet @ Geometry::SheetMetal { .. } => Some(sheet),
                _ => None,
            }),
        _ => None,
    }
}

/// Read the sheet recipe and preserve the parameter expressions.
pub fn recipe<'a>(data: &'a ProjectData, part: &str) -> Option<&'a Geometry<Scalar>> {
    let parts = &data.parts;
    let item = parts.get(part)?;
    let owner = match &item.geometry {
        Geometry::Instance { of } => parts.get(of)?,
        _ => item,
    };
    output(&owner.geometry)
}

fn resolved<'a>(
    project: &'a Project,
    part: &str,
) -> Result<(&'a SheetRule, &'a SketchProfile, &'a [Flange])> {
    let (geometry, _) = project.local_geometry(part)?;
    let Some(Geometry::SheetMetal {
        rule,
        base,
        flanges,
    }) = output(geometry)
    else {
        return Err(format!(
            "Part {part} needs a sheet metal recipe at its output."
        ));
    };
    Ok((rule, base, flanges))
}

pub fn pattern(project: &Project, part: &str) -> Result<FlatPattern> {
    project.cached("flat_pattern", part, || {
        faber_sheet::flatten(resolved(project, part)?)
    })
}

#[derive(Clone, Debug)]
pub struct FlangeEdge {
    pub wall: String,
    pub edge: u32,
    pub ends: [[f64; 3]; 2],
    pub other: [[f64; 3]; 2],
}

fn free_edges(
    folded: &faber_sheet::Folded,
    flanges: &[Flange],
    thickness: f64,
    placement: glam::DAffine3,
) -> Vec<FlangeEdge> {
    let mut out = Vec::new();
    for wall in &folded.walls {
        let frame = placement * wall.frame;
        for &(edge, line) in &wall.straight_edges {
            // Edge 1 of a flange joins the bend to its parent.
            if (wall.id != "base" && edge == 1)
                || flanges.iter().any(|f| f.on == wall.id && f.edge == edge)
            {
                continue;
            }
            let at = |z| line.map(|[x, y]| frame.transform_point3(DVec3::new(x, y, z)).to_array());
            out.push(FlangeEdge {
                wall: wall.id.clone(),
                edge,
                ends: at(0.),
                other: at(thickness),
            });
        }
    }
    out
}

/// Read the visible sheet from its current recipe without loading assembly meshes.
pub fn inspect(data: &ProjectData, part: &str) -> Result<(FlatPattern, Vec<FlangeEdge>)> {
    let source = recipe(data, part)
        .ok_or_else(|| format!("Part {part} needs a sheet metal recipe at its output."))?;
    let Geometry::SheetMetal {
        rule,
        base,
        flanges,
    } = faber_core::resolve_recipe(data, source)?
    else {
        return Err(format!("Part {part} has no sheet metal recipe."));
    };
    let tree = faber_sheet::build_for_flat(&rule, &base, &flanges)?;
    let frame = faber_core::world_placement(data, part)?;
    Ok((
        faber_sheet::flat::develop(&tree)?,
        free_edges(&tree, &flanges, rule.thickness, frame),
    ))
}

pub fn export(flat: &FlatPattern, title: &str, path: &Path) -> Result<()> {
    if let Some(problem) = flat.problems.first() {
        return Err(format!("Cannot export the flat pattern. {problem}"));
    }
    faber_files::VectorWriter.write_flat_pattern(&faber_sheet::to_vector(flat, title), path)
}

pub fn report(project: &Project, part: &str, path: Option<&Path>) -> Result<Value> {
    let flat = pattern(project, part)?;
    let (rule, _, _) = resolved(project, part)?;
    if let Some(path) = path {
        export(&flat, &project.data.parts[part].name, path)?;
    }
    let bends = flat
        .bends
        .iter()
        .map(|b| {
            json!({"flange":b.flange,"angle_deg":b.angle_deg,
        "direction":match b.direction {Direction::Up=>"up",Direction::Down=>"down"},
        "radius":b.radius,"allowance":b.allowance,"line":b.line})
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"part":part,"revision":project.data.revision,"method":"Develop the neutral surface with a constant K factor.",
        "resolution":"Bend allowances, complete circles and retained arcs are analytical. Other curved outlines use a 0.002 mm chord tolerance.",
        "k_factor":rule.k_factor.unwrap_or(0.44),"thickness_mm":rule.thickness,"size_mm":flat.size_mm,"area_mm2":flat.area_mm2,
        "cut_length_mm":flat.cut_length_mm,"bends":bends,"warnings":flat.warnings,"problems":flat.problems,
        "limits":["The K factor belongs to the sheet rule. Confirm it for the material and bend process."],"path":path}),
    )
}
