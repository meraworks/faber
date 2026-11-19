//! Write a sequence of RGBA frames.
use crate::Result;
use std::path::Path;

pub struct Sequence {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub fps: u32,
}
pub trait AnimationWriter {
    fn write(
        &self,
        path: &Path,
        frames_dir: Option<&Path>,
        sequence: &Sequence,
        draw: &mut dyn FnMut(u32) -> Result<Vec<u8>>,
    ) -> Result<()>;
}
