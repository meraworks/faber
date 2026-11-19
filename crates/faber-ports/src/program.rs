use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Move {
    Rapid { to: [f64; 3] },
    Feed { to: [f64; 3], feed: f64 },
    Dwell { seconds: f64 },
}
impl Move {
    pub fn target(&self) -> Option<[f64; 3]> {
        match self {
            Self::Rapid { to } | Self::Feed { to, .. } => Some(*to),
            Self::Dwell { .. } => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    Generic,
    Grbl,
    LinuxCnc,
    GrblLaser,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub tool_number: u32,
    pub speed: f64,
    pub laser: bool,
    pub coolant: bool,
    pub moves: Vec<Move>,
}
pub struct Program {
    pub name: String,
    pub sections: Vec<Section>,
    pub warnings: Vec<String>,
}
pub trait ProgramExport: Send + Sync {
    fn write(&self, program: &Program, dialect: Dialect, path: &Path) -> Result<()>;
}
