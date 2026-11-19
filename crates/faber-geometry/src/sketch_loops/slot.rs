use faber_model::{SketchLoop, SketchSegment};
use faber_ports::Result;

pub fn boundary(centre: [f64; 2], length: f64, width: f64, rotation: f64) -> Result<SketchLoop> {
    super::positive(length)?;
    super::positive(width)?;
    if length < width {
        return Err("Slot length must be at least its width".into());
    }
    if !rotation.is_finite() || centre.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
        return Err("Slot centre and rotation must be finite and in range".into());
    }
    let radius = width / 2.;
    let half = (length - width) / 2.;
    if half == 0. {
        return Ok(SketchLoop::Circle { centre, radius });
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let point = |x: f64, y: f64| [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos];
    let start = point(-half, -radius);
    Ok(SketchLoop::Path {
        start,
        segments: vec![
            SketchSegment::Line {
                end: point(half, -radius),
            },
            SketchSegment::Arc {
                via: point(half + radius, 0.),
                end: point(half, radius),
            },
            SketchSegment::Line {
                end: point(-half, radius),
            },
            SketchSegment::Arc {
                via: point(-half - radius, 0.),
                end: start,
            },
        ],
    })
}
