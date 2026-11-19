//! Static stress analysis in millimetres, newtons and megapascals.

mod grid;
pub use grid::{CELL_LIMIT, Grid, cells_near, default_h, voxelise, voxelise_with_progress};

mod eigen;
pub mod element;
pub mod modal;
pub mod solve;
pub mod stress;
pub mod thermal;
mod thermal_strain;
pub use element::element_matrix;
pub use modal::{MODAL_FREE_LIMIT, ModalCase, Mode, solve_modal, solve_modal_with_progress};
pub use solve::{
    Elastic, Load, StaticCase, StaticResult, Support, solve_static, solve_static_with_progress,
};
pub mod shape;
pub mod surface;
pub use shape::{ShapeCase, ShapeResult, optimise_shape, optimise_shape_with_progress};
pub use surface::iso_surface;
