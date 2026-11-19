//! Toolpaths in setup coordinates. Lengths use millimetres and feeds use millimetres per minute.
pub mod drill;
pub mod path;
pub mod pocket;
pub mod region;
pub mod two_d;

pub use drill::{Hole, drill, find_holes};
pub use faber_ports::program::Move;
pub use path::{StockBox, Toolpath, validate_stock_depth};
pub use pocket::{pocket, pocket_on_floor};
pub use region::{Region, offset, regions_at};
pub use two_d::{contour, contour_on_face, cut2d, face};

pub mod height_map;
pub mod three_d;
pub mod verify;
pub use height_map::{HeightMap, height_map, hidden_area, stock_height_map, tool_surface};
pub use three_d::{parallel3d, rough3d};
pub use verify::{OperationVerification, Verification, stock_mesh, verify, verify_operations};
