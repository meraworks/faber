//! Raster preview with the sheet aspect ratio and a white background.
use super::paper;
use faber_ports::{Result, vector::VectorDoc};
use tiny_skia::{Color, FillRule, Paint, Pixmap, Stroke, StrokeDash, Transform};

pub(super) fn render(doc: &VectorDoc, width: u32) -> Result<Vec<u8>> {
    let scale = f64::from(width) / doc.size_mm[0];
    let height = (doc.size_mm[1] * scale).round().max(1.);
    if width == 0
        || width > 16384
        || height > 16384.
        || f64::from(width) * height > 32_000_000.
        || !(scale as f32).is_finite()
    {
        return Err("Use a positive width and a sheet of at most 32 million pixels and 16384 pixels per side.".into());
    }
    let mut pixmap =
        Pixmap::new(width, height as u32).ok_or("Cannot allocate the drawing preview.")?;
    pixmap.fill(Color::WHITE);
    let transform = Transform::from_row(scale as f32, 0., 0., -(scale as f32), 0., height as f32);
    for layer in &doc.layers {
        let mut paint = Paint::default();
        let [r, g, b] = layer.colour;
        paint.set_color_rgba8(r, g, b, 255);
        let stroke = Stroke {
            width: layer.width_mm as f32,
            dash: if layer.dashed {
                StrokeDash::new(vec![4., 2.], 0.)
            } else {
                None
            },
            ..Default::default()
        };
        for path in &layer.paths {
            pixmap.stroke_path(&paper::path(path)?, &paint, &stroke, transform, None);
        }
        for label in doc.texts.iter().filter(|l| l.layer == layer.name) {
            if let Some(path) = paper::outline(label)? {
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }
    }
    pixmap
        .encode_png()
        .map_err(|e| format!("Cannot encode the drawing preview: {e}"))
}
