//! A design source: a text file that generates the project files, for
//! example a Jsonnet design.

use crate::Result;
use serde::Serialize;
use std::path::Path;

/// The result of a build.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub source: String,
    /// Files that the build wrote because their content changed.
    pub written: Vec<String>,
    /// The number of parts after the build.
    pub parts: usize,
    pub added_parts: Vec<String>,
    /// Parts that the source no longer has. Their placement is removed.
    pub removed_parts: Vec<String>,
    pub warnings: Vec<String>,
    /// Every file the build read.
    pub inputs: Vec<String>,
}

/// Generates the project files. The source owns `faber.json` and the part files.
pub trait DesignSource: Send + Sync {
    /// The file name of the source in a project folder, for example `design.jsonnet`.
    fn file_name(&self) -> &str;
    /// True when the project folder has a source.
    fn has_source(&self, root: &Path) -> bool {
        root.join(self.file_name()).is_file()
    }
    /// Build when an input changed. Return `None` if nothing was built.
    fn ensure_built(&self, root: &Path) -> Result<Option<Report>>;
    /// Build now. A failed build writes no file.
    fn build(&self, root: &Path) -> Result<Report>;
    /// A starter source for a new project.
    fn template(&self, name: &str) -> String;
}
