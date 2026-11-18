//! The [`DesignSource`] port, implemented with the Jsonnet build of this crate.

use std::path::Path;
use std::sync::Arc;

use faber_ports::source::{DesignSource, Report};
use faber_ports::{Kernel, Result};

/// A `design.jsonnet` source in the project folder. Each build checks the
/// parts with the kernel before any file changes.
#[derive(Clone)]
pub struct JsonnetSource {
    kernel: Arc<dyn Kernel>,
}

impl JsonnetSource {
    pub fn new(kernel: Arc<dyn Kernel>) -> Self {
        Self { kernel }
    }
}

impl DesignSource for JsonnetSource {
    fn file_name(&self) -> &str {
        crate::SOURCE_FILE
    }

    fn ensure_built(&self, root: &Path) -> Result<Option<Report>> {
        crate::ensure_built(self.kernel.clone(), root)
    }

    fn build(&self, root: &Path) -> Result<Report> {
        crate::build(self.kernel.clone(), root)
    }

    fn template(&self, name: &str) -> String {
        crate::template(name)
    }
}
