//! Static stress analysis in millimetres, newtons and megapascals.

mod grid;
pub use grid::{CELL_LIMIT, Grid, cells_near, default_h, voxelise};

pub mod element;
pub mod solve;
pub mod stress;
pub use element::element_matrix;
pub use solve::{
    Elastic, Load, StaticCase, StaticResult, Support, solve_static, solve_static_with_progress,
};
