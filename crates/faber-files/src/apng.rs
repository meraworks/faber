//! Write an animation a frame at a time.
use faber_ports::Result;
use faber_ports::animation::{AnimationWriter, Sequence};
use std::path::Path;

pub struct Writer;
impl AnimationWriter for Writer {
    fn write(
        &self,
        path: &Path,
        frames_dir: Option<&Path>,
        sequence: &Sequence,
        draw: &mut dyn FnMut(u32) -> Result<Vec<u8>>,
    ) -> Result<()> {
        write(
            path,
            frames_dir,
            sequence.width,
            sequence.height,
            sequence.frames,
            sequence.fps,
            draw,
        )
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

pub fn write(
    path: &Path,
    frames_dir: Option<&Path>,
    width: u32,
    height: u32,
    frames: u32,
    fps: u32,
    mut draw: impl FnMut(u32) -> Result<Vec<u8>>,
) -> Result<()> {
    if frames < 2 {
        return Err("An animation needs 2 frames or more.".into());
    }
    if frames > 600 || !(1..=240).contains(&fps) {
        return Err("Use at most 600 frames and 1 to 240 frames per second.".into());
    }
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Err("The image dimensions must be between 1 and 4096 pixels.".into());
    }
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("png") && !ext.eq_ignore_ascii_case("apng") {
        return Err("Use a .png or .apng file for the animation.".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent(path)).map_err(|e| e.to_string())?;
    let stage = frames_dir
        .map(|dir| {
            if dir.exists() || dir == path {
                return Err("Choose a new directory for the separate frames.".to_owned());
            }
            tempfile::tempdir_in(parent(dir)).map_err(|e| e.to_string())
        })
        .transpose()?;
    {
        let mut encoder = png::Encoder::new(temp.as_file_mut(), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(frames, 0).map_err(|e| e.to_string())?;
        encoder
            .set_frame_delay(1, fps as u16)
            .map_err(|e| e.to_string())?;
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        for i in 0..frames {
            let pixels = draw(i)?;
            if pixels.len() != width as usize * height as usize * 4 {
                return Err(format!("Frame {} has the wrong pixel count.", i + 1));
            }
            writer
                .write_image_data(&pixels)
                .map_err(|e| e.to_string())?;
            if let Some(stage) = &stage {
                let file = std::fs::File::create(stage.path().join(format!("{:04}.png", i + 1)))
                    .map_err(|e| e.to_string())?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut out = enc.write_header().map_err(|e| e.to_string())?;
                out.write_image_data(&pixels).map_err(|e| e.to_string())?;
                out.finish().map_err(|e| e.to_string())?;
            }
        }
        writer.finish().map_err(|e| e.to_string())?;
    }
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    if let (Some(stage), Some(dest)) = (&stage, frames_dir) {
        std::fs::rename(stage.path(), dest).map_err(|e| e.to_string())?;
    }
    if let Err(err) = temp.persist(path) {
        if let (Some(stage), Some(dest)) = (&stage, frames_dir) {
            std::fs::rename(dest, stage.path())
                .map_err(|e| format!("{}; cannot restore the frame directory: {e}", err.error))?;
        }
        return Err(err.error.to_string());
    }
    Ok(())
}
