use super::*;
use faber_core::machining::Edit;
use faber_model::machining::{Operation, Setup, Tool};
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTool {
    pub id: String,
    #[schemars(with = "serde_json::Value")]
    pub definition: Tool,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSetup {
    pub id: String,
    #[schemars(with = "serde_json::Value")]
    pub setup: Setup,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetOperation {
    #[schemars(with = "serde_json::Value")]
    pub operation: Box<Operation>,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveOperation {
    pub id: String,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub setup: String,
    pub operations: Option<Vec<String>>,
    pub resolution_mm: Option<f64>,
    pub render: Option<Image>,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramFile {
    pub setup: String,
    pub path: PathBuf,
    pub dialect: Option<String>,
    #[serde(default)]
    pub force: bool,
    pub expected_revision: Option<u64>,
}

impl Faber {
    pub(super) fn machining_edit(&self, revision: Option<u64>, edit: Edit) -> CallToolResult {
        self.with(|p| {
            check_revision(p, revision)?;
            p.batch_operations(
                vec![faber_core::Operation::Machining(Box::new(edit))],
                false,
            )?;
            Ok(json!({"revision":p.data.revision}))
        })
    }
}
