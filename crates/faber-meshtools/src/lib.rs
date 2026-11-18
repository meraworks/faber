//! Inspect and edit triangle meshes.
mod cut;
mod reduce;
mod repair;
mod report;
mod topology;

pub use cut::{Keep, cut, slice};
pub use reduce::{ReduceLog, Target, reduce};
pub use repair::{RepairLog, RepairOptions, repair, try_repair};
pub use report::{MeshReport, report, try_report};
pub use topology::{Weld, try_weld, weld};
