//! Sheet paths and letter outlines for paper and pixel output.
use faber_ports::{
    Result,
    vector::{Anchor, Path2, Text},
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    outline::OutlinePen,
};
use tiny_skia::{Path, PathBuilder};

pub(super) fn path(shape: &Path2) -> Result<Path> {
    let mut b = PathBuilder::new();
    match shape {
        Path2::Polyline { points, closed } => {
            b.move_to(points[0][0] as f32, points[0][1] as f32);
            for p in &points[1..] {
                b.line_to(p[0] as f32, p[1] as f32);
            }
            if *closed {
                b.close();
            }
        }
        Path2::Circle { centre, radius } => circle_arc(&mut b, *centre, *radius, 0., 360., true),
        Path2::Arc {
            centre,
            radius,
            start_deg,
            end_deg,
        } => {
            let start = start_deg.rem_euclid(360.);
            circle_arc(
                &mut b,
                *centre,
                *radius,
                start,
                (end_deg.rem_euclid(360.) - start).rem_euclid(360.),
                false,
            );
        }
    }
    b.finish()
        .ok_or_else(|| "The drawing path is outside the supported range.".into())
}

fn circle_arc(b: &mut PathBuilder, c: [f64; 2], r: f64, start: f64, span: f64, closed: bool) {
    let steps = (span / 90.).ceil() as usize;
    let step = span.to_radians() / steps as f64;
    let k = (step / 4.).tan() * 4. / 3.;
    let point = |x: f64, y: f64| ((c[0] + r * x) as f32, (c[1] + r * y) as f32);
    let (s, t) = start.to_radians().sin_cos();
    let p = point(t, s);
    b.move_to(p.0, p.1);
    for i in 0..steps {
        let angle = start.to_radians() + i as f64 * step;
        let (s0, c0) = angle.sin_cos();
        let (s1, c1) = (angle + step).sin_cos();
        let a = point(c0 - k * s0, s0 + k * c0);
        let z = point(c1 + k * s1, s1 - k * c1);
        let p = point(c1, s1);
        b.cubic_to(a.0, a.1, z.0, z.1, p.0, p.1);
    }
    if closed {
        b.close();
    }
}

pub(super) fn anchor_offset(anchor: Anchor, width: f64) -> f64 {
    match anchor {
        Anchor::Start => 0.,
        Anchor::Centre => -width / 2.,
        Anchor::End => -width,
    }
}

struct Pen {
    path: PathBuilder,
    x: f32,
    y: f32,
    scale: f32,
}
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path
            .move_to(self.x + x * self.scale, self.y + y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path
            .line_to(self.x + x * self.scale, self.y + y * self.scale);
    }
    fn quad_to(&mut self, x: f32, y: f32, u: f32, v: f32) {
        self.path.quad_to(
            self.x + x * self.scale,
            self.y + y * self.scale,
            self.x + u * self.scale,
            self.y + v * self.scale,
        );
    }
    fn curve_to(&mut self, x: f32, y: f32, u: f32, v: f32, a: f32, b: f32) {
        self.path.cubic_to(
            self.x + x * self.scale,
            self.y + y * self.scale,
            self.x + u * self.scale,
            self.y + v * self.scale,
            self.x + a * self.scale,
            self.y + b * self.scale,
        );
    }
    fn close(&mut self) {
        self.path.close();
    }
}

pub(super) fn outline(label: &Text) -> Result<Option<Path>> {
    let font = FontRef::new(epaint_default_fonts::UBUNTU_LIGHT).map_err(|e| e.to_string())?;
    let size = Size::unscaled();
    let loc = LocationRef::default();
    let scale = label.height_mm as f32 / font.metrics(size, loc).cap_height.unwrap_or(700.);
    let advances = font.glyph_metrics(size, loc);
    let cmap = font.charmap();
    let shapes = font.outline_glyphs();
    let mut glyphs = Vec::new();
    let mut width = 0.;
    for ch in label.text.chars() {
        let id = cmap
            .map(ch)
            .ok_or_else(|| format!("The drawing font has no letter for {ch}."))?;
        let step = advances.advance_width(id).unwrap_or(0.) * scale;
        width += step;
        glyphs.push((id, step));
    }
    let mut pen = Pen {
        path: PathBuilder::new(),
        x: (label.at[0] + anchor_offset(label.anchor, width.into())) as f32,
        y: label.at[1] as f32,
        scale,
    };
    for (id, step) in glyphs {
        if let Some(glyph) = shapes.get(id) {
            glyph.draw(size, &mut pen).map_err(|e| e.to_string())?;
        }
        pen.x += step;
    }
    if pen.path.is_empty() {
        return Ok(None);
    }
    pen.path
        .finish()
        .map(Some)
        .ok_or_else(|| "The drawing text is outside the supported range.".into())
}
