//! Mesh checks before slicing.
mod check;
mod fit;
mod footprint;
mod mesh;
mod orient;
mod support;
mod wall;

pub use check::{Finding, Kind, PrintReport, Resolution, Severity, check, check_with_walls};
pub use fit::{CylinderFit, FitClass, Stack, StackItem, checked_stack, cylinder_fits, stack};
pub use orient::{Candidate, Weights, rank};
pub use support::{islands, support_volume};
pub use wall::{is_thin_wall, wall_thickness};
