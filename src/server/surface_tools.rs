//! Read surface measurements and save their colour maps.
use super::*;
use faber_core::surface::CurvatureMode;
use faber_wiring::surface::{self, Request};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldImage {
    path: PathBuf,
    mode: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Curvature {
    part: String,
    tool_radius_mm: Option<f64>,
    detail: Option<bool>,
    #[serde(alias = "resolution_mm")]
    resolution: Option<f64>,
    render: Option<FieldImage>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftAnalysis {
    part: String,
    pull: [f64; 3],
    min_deg: Option<f64>,
    detail: Option<bool>,
    #[serde(alias = "resolution_mm")]
    resolution: Option<f64>,
    render: Option<FieldImage>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessAnalysis {
    part: String,
    directions: Vec<[f64; 3]>,
    scene: Option<bool>,
    detail: Option<bool>,
    #[serde(alias = "resolution_mm")]
    resolution: Option<f64>,
    render: Option<FieldImage>,
}

pub(super) fn curvature(p: &Project, a: Curvature) -> Result<Value, String> {
    let mode = match a
        .render
        .as_ref()
        .and_then(|r| r.mode.as_deref())
        .unwrap_or("mean")
    {
        "mean" => CurvatureMode::Mean,
        "gauss" => CurvatureMode::Gauss,
        "min_radius" => CurvatureMode::MinRadius,
        _ => return Err("Choose mean, gauss or min_radius for the curvature map.".into()),
    };
    read(
        p,
        &a.part,
        Request::Curvature {
            mode,
            tool_radius_mm: a.tool_radius_mm,
        },
        a.resolution,
        a.detail,
        a.render,
    )
}
pub(super) fn draft(p: &Project, a: DraftAnalysis) -> Result<Value, String> {
    read(
        p,
        &a.part,
        Request::Draft {
            pull: a.pull,
            min_deg: a.min_deg.unwrap_or(1.),
        },
        a.resolution,
        a.detail,
        a.render,
    )
}
pub(super) fn access(p: &Project, a: AccessAnalysis) -> Result<Value, String> {
    read(
        p,
        &a.part,
        Request::Access {
            directions: a.directions,
            scene: a.scene.unwrap_or(false),
        },
        a.resolution,
        a.detail,
        a.render,
    )
}

fn read(
    p: &Project,
    part: &str,
    request: Request,
    resolution: Option<f64>,
    detail: Option<bool>,
    render: Option<FieldImage>,
) -> Result<Value, String> {
    if !matches!(request, Request::Curvature { .. })
        && render.as_ref().is_some_and(|r| r.mode.is_some())
    {
        return Err("A map mode applies only to curvature.".into());
    }
    let field = surface::cached(p, part, &request, resolution)?;
    let mut value = field.report.clone();
    if detail != Some(true) {
        let summary = value.as_object_mut().expect("Surface report fields");
        summary.remove("faces");
        for key in [
            "smallest_concave_faces",
            "hidden_faces",
            "tool_too_large_faces",
        ] {
            if let Some(list) = summary.get_mut(key).and_then(Value::as_array_mut) {
                let count = list.len();
                list.truncate(16);
                summary.insert(format!("{key}_count"), json!(count));
            }
        }
    }
    if let Some(render) = render {
        let body = p
            .bodies
            .iter()
            .find(|b| b.id == part)
            .ok_or("The selected part is missing.")?;
        surface::save_image(body, &field, &render.path)?;
        value["render"] = json!(render.path);
    }
    value["revision"] = json!(p.data.revision);
    Ok(value)
}
