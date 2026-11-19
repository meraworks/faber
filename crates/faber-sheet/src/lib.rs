//! Flat walls joined by constant-radius bends.
use faber_model::{Flange, SheetRule, SketchLoop, SketchProfile};
use glam::{DAffine2, DAffine3};

mod collision;
mod fold;
mod tree;

pub type Edge = (u32, [[f64; 2]; 2]);
#[derive(Clone, Debug)]
pub struct Wall {
    pub(crate) edge_count: usize,
    pub(crate) ccw: bool,
    pub id: String,
    pub outline: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
    pub frame: DAffine3,
    pub flat_frame: DAffine2,
    pub profile: SketchProfile,
    pub reliefs: Vec<SketchLoop>,
    pub straight_edges: Vec<Edge>,
}
#[derive(Clone, Debug)]
pub struct Bend {
    pub flange: String,
    pub line: [[f64; 3]; 2],
    pub section: [[f64; 3]; 4],
    pub angle_deg: f64,
    pub radius: f64,
    pub allowance: f64,
    pub width: f64,
    pub flat_zone: [[f64; 2]; 4],
    pub flat_line: [[f64; 2]; 2],
}
#[derive(Clone, Debug)]
pub struct Folded {
    pub walls: Vec<Wall>,
    pub bends: Vec<Bend>,
    pub warnings: Vec<String>,
}
pub fn fold(rule: &SheetRule, base: &SketchProfile, flanges: &[Flange]) -> Result<Folded, String> {
    fold::build(rule, base, flanges, true)
}
pub fn build_for_flat(
    rule: &SheetRule,
    base: &SketchProfile,
    flanges: &[Flange],
) -> Result<Folded, String> {
    fold::build(rule, base, flanges, false)
}
pub fn sample_loop(source: &SketchLoop) -> Result<Vec<[f64; 2]>, String> {
    tree::sample_loop(source)
}
pub mod flat;
pub use flat::{BendLine, Direction, FlatPattern, flatten, to_vector};
