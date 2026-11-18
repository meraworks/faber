//! Parts and total masses of one assembly.
use serde::Serialize;
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BomRow {
    pub item: u32,
    pub quantity: u32,
    pub name: String,
    pub parts: Vec<String>,
    pub material: Option<String>,
    pub mass_g: Option<f64>,
    pub source: BomSource,
    pub component: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BomSource {
    Made,
    Purchased {
        designation: String,
        source: String,
        licence: String,
    },
}
pub trait BillWriter {
    fn write(&self, rows: &[BomRow]) -> String;
}
