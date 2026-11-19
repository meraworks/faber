//! Closed control cages and their smooth subdivision surfaces.
mod cage;
pub mod edit;
pub mod primitive_input;
mod primitives;
mod subdivide;

pub use cage::{Cage, MAX_FACES};
pub use edit::{FormOp, Select, apply, check_symmetry};
pub use faber_model::{CageData, Crease, SymmetryPlane};
pub use primitives::*;
pub use subdivide::{subdivide, subdivide_once, subdivide_topology};
