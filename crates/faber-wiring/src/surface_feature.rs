pub use faber_core::surface_feature::Display;
use faber_files::{FileAssets, FileFeatureCache};
use faber_model::ProjectData;
use faber_ports::Result;
use std::path::Path;

/// Read one placed surface without loading other parts.
pub fn display(root: &Path, data: &ProjectData, part: &str, feature: &str) -> Result<Display> {
    faber_core::surface_feature::display(
        data,
        part,
        feature,
        &crate::OcctKernel,
        &FileAssets::new(root),
        &FileFeatureCache::read_only(root),
    )
}
