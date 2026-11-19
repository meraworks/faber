//! Planar sketch constraints.
pub use faber_model::{
    SketchConstraint as Constraint, SketchEntity as Entity, SolvedSketchData as Sketch,
};
pub type Result<T> = std::result::Result<T, String>;
pub use solve::{Solution, Status, drag, solve};
mod dense;
pub mod edit;
pub mod profile;
mod residual;
mod solve;
pub use edit::{SketchOp, apply};
