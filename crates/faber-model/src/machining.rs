//! Tools and operations in setup coordinates. Distances are in millimetres.
use crate::FaceSelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Machining {
    #[serde(default)]
    pub tools: BTreeMap<String, Tool>,
    #[serde(default)]
    pub setups: BTreeMap<String, Setup>,
    #[serde(default)]
    pub operations: Vec<Operation>,
}
impl Machining {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.setups.is_empty() && self.operations.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub kind: ToolKind,
    pub diameter: f64,
    #[serde(default)]
    pub corner_radius: f64,
    pub flute_length: f64,
    pub flutes: u32,
    pub feed_mm_min: f64,
    pub plunge_mm_min: f64,
    pub spindle_rpm: f64,
    pub number: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Flat,
    Ball,
    Bull,
    Drill,
    Chamfer,
    Laser,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub part: String,
    pub stock: Stock,
    pub origin: Origin,
    pub up: [f64; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Stock {
    Offset { sides: f64, top: f64, bottom: f64 },
    Box { size: [f64; 3] },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    StockTopCentre,
    StockTopCorner,
    PartBottomCorner,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Heights {
    pub clearance: f64,
    pub retract: f64,
    pub top: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    pub id: String,
    pub setup: String,
    pub tool: String,
    pub heights: Heights,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    #[serde(flatten)]
    pub kind: OperationKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationKind {
    Face {
        stepover: f64,
    },
    Contour {
        z: Selection,
        side: Side,
        #[serde(default)]
        stock_to_leave: f64,
    },
    Pocket {
        floor: Selection,
        stepover: f64,
        #[serde(default)]
        stock_to_leave: f64,
    },
    Drill {
        holes: Holes,
        #[serde(skip_serializing_if = "Option::is_none")]
        peck: Option<f64>,
    },
    Rough3d {
        stepover: f64,
        #[serde(default)]
        stock_to_leave: f64,
    },
    Parallel3d {
        stepover: f64,
        angle_deg: f64,
    },
    Cut2d {
        kerf: f64,
        lead_in: f64,
        power: f64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    Z(f64),
    Face(FaceSelector),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Outside,
    Inside,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Holes {
    Auto,
    List(Vec<[f64; 2]>),
}
