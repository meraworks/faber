use super::*;
use faber_model::SymmetryPlane;
use faber_wiring::form::{self, FormOp, Primitive};

#[derive(Deserialize, JsonSchema)]
pub struct NewForm {
    id: String,
    primitive: String,
    levels: Option<u32>,
    #[serde(default)]
    as_solid: bool,
    #[serde(default)]
    symmetry: Vec<SymmetryPlane>,
    expected_revision: Option<u64>,
    #[serde(flatten)]
    dimensions: serde_json::Map<String, Value>,
}
impl NewForm {
    pub fn create(self, p: &mut Project) -> Result<Value, String> {
        let rev = self.expected_revision.unwrap_or(p.data.revision);
        p.check_edit_revision(rev)?;
        let mut fields = self.dimensions;
        fields.insert("primitive".into(), Value::String(self.primitive));
        let primitive: Primitive = serde_json::from_value(Value::Object(fields))
            .map_err(|e| format!("Invalid form primitive: {e}"))?;
        form::new_form(
            p,
            &self.id,
            primitive.cage()?,
            self.levels,
            self.as_solid,
            self.symmetry,
            rev,
        )?;
        changed(p, &self.id, None)
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditForm {
    part: String,
    feature: Option<String>,
    #[schemars(with = "Vec<Value>")]
    ops: Vec<FormOp>,
    expected_revision: Option<u64>,
}
impl EditForm {
    pub fn apply(self, p: &mut Project) -> Result<Value, String> {
        let feature = self.feature.as_deref();
        let rev = self.expected_revision.unwrap_or(p.data.revision);
        form::edit_form(p, &self.part, feature, &self.ops, rev)?;
        changed(p, &self.part, feature)
    }
}
fn changed(p: &Project, part: &str, feature: Option<&str>) -> Result<Value, String> {
    let mut summary = form::info(p, part, feature, false)?;
    if let Some(body) = p.bodies.iter().find(|b| b.id == part) {
        summary["changed"] = part_summary(body);
    }
    summary["revision"] = json!(p.data.revision);
    Ok(summary)
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormInfo {
    pub part: String,
    pub feature: Option<String>,
    #[serde(default)]
    pub detail: bool,
}
