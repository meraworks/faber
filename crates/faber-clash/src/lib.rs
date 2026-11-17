//! Clash checking for mechanical assemblies.
//!
//! Given triangle meshes of the parts, a list of poses, and rules about which
//! pairs are allowed to touch or overlap, `faber-clash` reports for every pose
//! which pairs intersect (and how deep) or come closer than a proximity
//! distance.
//!
//! ```no_run
//! use faber_clash::{scan, Scene, ScanOptions};
//!
//! let scene = Scene::load("package".as_ref()).unwrap();
//! let report = scan(&scene, &ScanOptions::default()).unwrap();
//! println!("{} violations", report.violations.len());
//! ```
//!
//! Modules:
//! - [`package`]: on-disk format (`scene.json` + `scene.bin`) and loader
//! - [`scene`]: parts, poses and rules in memory
//! - [`query`]: pair queries (BVH traversal, penetration depth)
//! - [`scan`]: broad phase, parallel narrow phase, classification
//! - [`report`]: findings and JSON output
//!
//! There is no B-rep code here. Exact booleans belong to a CAD kernel; this
//! crate is a fast screening step that runs on tessellated geometry.

pub mod package;
pub mod query;
pub mod report;
pub mod scan;
pub mod scene;

pub use report::{Finding, FindingKind, Report, Status};
pub use scan::{ScanOptions, scan};
pub use scene::{Part, PartKind, Scene};
