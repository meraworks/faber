//! Stdio arguments for mesh edits.
use super::*;
use faber_wiring::mesh_edit::{self, Keep, MeshEdit, RepairOptions, Target};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshRead {
    pub(super) part: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairMesh {
    part: String,
    expected_revision: u64,
    #[serde(default)]
    options: RepairOptions,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReduceMesh {
    part: String,
    expected_revision: u64,
    triangles: Option<usize>,
    ratio: Option<f64>,
    max_error_mm: Option<f64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Plane {
    origin: [f64; 3],
    normal: [f64; 3],
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutMesh {
    part: String,
    expected_revision: u64,
    plane: Plane,
    keep: Keep,
    cap: Option<bool>,
    copy_as: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertMesh {
    part: String,
    expected_revision: u64,
    merge_planar: Option<bool>,
    copy_as: Option<String>,
}

fn edit(
    project: &mut Project,
    part: &str,
    revision: u64,
    operation: MeshEdit,
) -> Result<Value, String> {
    let edit = mesh_edit::apply(project, part, revision, operation)?;
    let changed: Vec<_> = project
        .bodies
        .iter()
        .filter(|b| edit.changed && edit.parts.contains(&b.id))
        .map(part_summary)
        .collect();
    Ok(
        json!({"schema":1,"revision":edit.revision,"changed":changed,"part_count":project.bodies.len(),"log":edit.log}),
    )
}
pub(super) fn repair(p: &mut Project, a: RepairMesh) -> Result<Value, String> {
    edit(p, &a.part, a.expected_revision, MeshEdit::Repair(a.options))
}
pub(super) fn reduce(p: &mut Project, a: ReduceMesh) -> Result<Value, String> {
    let target = match (a.triangles, a.ratio, a.max_error_mm) {
        (Some(n), None, None) => Target::Triangles(n),
        (None, Some(r), None) => Target::Ratio(r),
        (None, None, Some(e)) => Target::MaxError(e),
        _ => return Err("Set exactly one reduction target.".into()),
    };
    edit(p, &a.part, a.expected_revision, MeshEdit::Reduce(target))
}
pub(super) fn cut(p: &mut Project, a: CutMesh) -> Result<Value, String> {
    edit(
        p,
        &a.part,
        a.expected_revision,
        MeshEdit::Cut {
            origin: a.plane.origin,
            normal: a.plane.normal,
            keep: a.keep,
            cap: a.cap.unwrap_or(true),
            copy_as: a.copy_as,
        },
    )
}
pub(super) fn solid(p: &mut Project, a: ConvertMesh) -> Result<Value, String> {
    edit(
        p,
        &a.part,
        a.expected_revision,
        MeshEdit::ToSolid {
            merge_planar: a.merge_planar.unwrap_or(true),
            copy_as: a.copy_as,
        },
    )
}
