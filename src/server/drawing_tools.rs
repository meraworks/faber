use super::*;
use faber_model::drawing::{Drawing, Sheet};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDrawing {
    id: String,
    #[schemars(with = "serde_json::Value")]
    drawing: Drawing,
    expected_revision: u64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteDrawing {
    id: String,
    expected_revision: u64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoDrawing {
    id: String,
    parts: Option<Vec<String>>,
    sheet: Option<Sheet>,
    title: Option<String>,
    expected_revision: u64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportDrawing {
    id: String,
    pub(super) path: PathBuf,
    width_px: Option<u32>,
    #[serde(default)]
    pub(super) inline: bool,
}

pub(super) fn set(p: &mut Project, a: SetDrawing) -> Result<Value, String> {
    p.batch_at_revision(
        a.expected_revision,
        vec![Edit::SetDrawing {
            id: a.id.clone(),
            drawing: a.drawing,
        }],
    )?;
    Ok(json!({"id":a.id,"revision":p.data.revision}))
}
pub(super) fn remove(p: &mut Project, a: DeleteDrawing) -> Result<Value, String> {
    p.batch_at_revision(
        a.expected_revision,
        vec![Edit::RemoveDrawing { id: a.id.clone() }],
    )?;
    Ok(json!({"id":a.id,"revision":p.data.revision}))
}
pub(super) fn auto(p: &mut Project, a: AutoDrawing) -> Result<Value, String> {
    p.check_edit_revision(a.expected_revision)?;
    let drawing = faber_wiring::drawing::auto_drawing(
        p,
        &a.parts.unwrap_or_default(),
        a.sheet.unwrap_or_default(),
        a.title.as_deref().unwrap_or(&p.data.name),
    )?;
    set(
        p,
        SetDrawing {
            id: a.id,
            drawing,
            expected_revision: a.expected_revision,
        },
    )
}
pub(super) fn export(p: &Project, a: &ExportDrawing) -> Result<Value, String> {
    if a.inline
        && !a
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return Err("Use a PNG file for an inline image.".into());
    }
    let r = faber_wiring::drawing::export(p, &a.id, &a.path, a.width_px)?;
    Ok(
        json!({"id":a.id,"path":a.path,"revision":p.data.revision,"problems":r.problems,"limits":r.limits}),
    )
}
