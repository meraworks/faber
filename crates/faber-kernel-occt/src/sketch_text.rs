use faber_model::{Placement, SketchLoop, SketchProfile, TextAlign};
use faber_ports::{Combine, Kernel, Result, Shape};
use skrifa::{
    FontRef, MetadataProvider, instance::LocationRef, instance::Size, outline::OutlinePen,
};

/// A shape that `sketch` built from text regions. Only this shape extrudes region by region.
pub(crate) struct TextRegions(Shape);

/// Returns the shape under a text marker, or the shape itself.
pub(crate) fn unmarked(shape: &Shape) -> &Shape {
    shape.get::<TextRegions>().map_or(shape, |marked| &marked.0)
}

/// Gives a copy of a text shape the text marker, when `source` has it.
pub(crate) fn carried(source: &Shape, copy: Shape) -> Shape {
    if source.get::<TextRegions>().is_some() {
        Shape::new(TextRegions(copy))
    } else {
        copy
    }
}

/// Built-in fonts. The data is compiled into the program.
const FONTS: [(&str, &[u8]); 2] = [
    ("ubuntu-light", epaint_default_fonts::UBUNTU_LIGHT),
    ("hack", epaint_default_fonts::HACK_REGULAR),
];
const MAX_CHARACTERS: usize = 256;
const MAX_OUTLINES: usize = 256;
const MAX_STEPS: usize = 64;

/// Sketches one text loop as one face per outline region.
/// The regions are fused into one shape.
pub(crate) fn sketch<K: Kernel>(
    kernel: &K,
    profile: &SketchProfile,
    placement: &Placement,
) -> Result<Shape> {
    if !profile.holes.is_empty() {
        return Err("A text sketch cannot have extra holes".into());
    }
    let mut shape: Option<Shape> = None;
    for region in expand(&profile.outer)? {
        let face = kernel.sketch(&region, placement)?;
        shape = Some(match shape {
            None => face,
            Some(previous) => kernel.combine(&previous, &face, Combine::Union)?,
        });
    }
    let shape = shape.ok_or("Text has no visible outline")?;
    Ok(Shape::new(TextRegions(shape)))
}

/// Extrudes each face of a multi-region text sketch and fuses the solids.
/// Returns None for a shape that is not a text sketch with a solid-free group of faces.
pub(crate) fn extrude_regions<K: Kernel>(
    kernel: &K,
    shape: &Shape,
    vector: [f64; 3],
) -> Result<Option<Shape>> {
    let Some(marked) = shape.get::<TextRegions>() else {
        return Ok(None);
    };
    let shape = &marked.0;
    let count = kernel.face_count(shape)?;
    if count < 2 || kernel.properties(shape)?.solids > 0 {
        return Ok(None);
    }
    let mut result: Option<Shape> = None;
    for index in 1..=count {
        let face = kernel.face_shape(shape, index)?;
        let solid = kernel.extrude_shape(&face, vector)?;
        result = Some(match result {
            None => solid,
            Some(previous) => kernel.combine(&previous, &solid, Combine::Union)?,
        });
    }
    Ok(result)
}

/// Expands a text loop into closed polygon regions in sketch coordinates.
/// Each region is an outer loop with the holes inside it.
pub(crate) fn expand(text: &SketchLoop) -> Result<Vec<SketchProfile>> {
    let SketchLoop::Text {
        text,
        height,
        at,
        align,
        angle_deg,
        font,
    } = text
    else {
        return Err("Only a text loop can be expanded as text".into());
    };
    if text.is_empty() {
        return Err("Text is empty".into());
    }
    if text.chars().count() > MAX_CHARACTERS {
        return Err("Text has more than 256 characters".into());
    }
    if text.chars().any(char::is_control) {
        return Err("Text must be one line".into());
    }
    if !(1e-6..=1e6).contains(height) {
        return Err("Text height must be between 0.000001 and 1000000 mm".into());
    }
    let angle_deg = angle_deg.unwrap_or(0.);
    if at
        .iter()
        .chain([&angle_deg])
        .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("Text position is outside the supported range".into());
    }
    let font = font_data(font.as_deref())?;
    let font = FontRef::new(font).map_err(|error| format!("Cannot read text font: {error}"))?;
    let cap = font
        .metrics(Size::unscaled(), LocationRef::default())
        .cap_height
        .filter(|cap| *cap > 0.)
        .ok_or("Text font has no cap height")?;
    let scale = height / f64::from(cap);
    let charmap = font.charmap();
    let glyphs = font.outline_glyphs();
    let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let mut pen = Pen {
        contours: Vec::new(),
        current: Vec::new(),
        cursor: 0.,
        scale,
        tolerance: 0.002 * height,
    };
    for ch in text.chars() {
        let glyph = charmap
            .map(ch)
            .ok_or_else(|| format!("Text font has no glyph for {ch}"))?;
        if let Some(outline) = glyphs.get(glyph) {
            outline
                .draw(Size::unscaled(), &mut pen)
                .map_err(|error| format!("Cannot draw text glyph: {error}"))?;
            pen.finish();
        }
        pen.cursor += f64::from(advances.advance_width(glyph).unwrap_or(0.)) * scale;
    }
    let width = pen.cursor;
    let dx = match align {
        TextAlign::Left => 0.,
        TextAlign::Centre => -width / 2.,
        TextAlign::Right => -width,
    };
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let at = *at;
    let contours = pen
        .contours
        .into_iter()
        .filter_map(clean)
        .map(|points| {
            points
                .into_iter()
                .map(|[x, y]| {
                    let x = x + dx;
                    [at[0] + x * cos - y * sin, at[1] + x * sin + y * cos]
                })
                .collect()
        })
        .collect();
    let regions = nest(contours);
    if regions.is_empty() {
        return Err("Text has no visible outline".into());
    }
    if regions.len() > MAX_OUTLINES {
        return Err("Text has more than 256 outlines".into());
    }
    Ok(regions)
}

fn font_data(name: Option<&str>) -> Result<&'static [u8]> {
    let name = name.unwrap_or("ubuntu-light");
    FONTS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, data)| *data)
        .ok_or_else(|| format!("Unknown text font: {name}"))
}

/// Collects glyph contours in millimetres. Curves become line segments.
struct Pen {
    contours: Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
    cursor: f64,
    scale: f64,
    tolerance: f64,
}

impl Pen {
    fn point(&self, x: f32, y: f32) -> [f64; 2] {
        [
            self.cursor + f64::from(x) * self.scale,
            f64::from(y) * self.scale,
        ]
    }

    fn last(&self) -> [f64; 2] {
        self.current.last().copied().unwrap_or([self.cursor, 0.])
    }

    fn finish(&mut self) {
        if !self.current.is_empty() {
            self.contours.push(std::mem::take(&mut self.current));
        }
    }

    fn steps(&self, factor: f64) -> usize {
        ((factor / self.tolerance).sqrt().ceil() as usize).clamp(1, MAX_STEPS)
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        let point = self.point(x, y);
        self.current.push(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = self.point(x, y);
        self.current.push(point);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, c, p2) = (self.last(), self.point(cx, cy), self.point(x, y));
        let second = norm([p0[0] - 2. * c[0] + p2[0], p0[1] - 2. * c[1] + p2[1]]);
        let steps = self.steps(second / 4.);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1. - t;
            self.current.push(std::array::from_fn(|k| {
                u * u * p0[k] + 2. * u * t * c[k] + t * t * p2[k]
            }));
        }
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let p0 = self.last();
        let (c1, c2, p3) = (self.point(c1x, c1y), self.point(c2x, c2y), self.point(x, y));
        let d1 = norm([p0[0] - 2. * c1[0] + c2[0], p0[1] - 2. * c1[1] + c2[1]]);
        let d2 = norm([c1[0] - 2. * c2[0] + p3[0], c1[1] - 2. * c2[1] + p3[1]]);
        let steps = self.steps(3. * d1.max(d2) / 4.);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1. - t;
            self.current.push(std::array::from_fn(|k| {
                u * u * u * p0[k]
                    + 3. * u * u * t * c1[k]
                    + 3. * u * t * t * c2[k]
                    + t * t * t * p3[k]
            }));
        }
    }

    fn close(&mut self) {
        self.finish();
    }
}

fn norm(v: [f64; 2]) -> f64 {
    v[0].hypot(v[1])
}

/// Removes repeated points and the closing duplicate. Drops a degenerate loop.
fn clean(mut points: Vec<[f64; 2]>) -> Option<Vec<[f64; 2]>> {
    points.dedup_by(|a, b| norm([a[0] - b[0], a[1] - b[1]]) < 1e-9);
    while points.len() > 1 {
        let (first, last) = (points[0], points[points.len() - 1]);
        if norm([first[0] - last[0], first[1] - last[1]]) >= 1e-9 {
            break;
        }
        points.pop();
    }
    (points.len() >= 3).then_some(points)
}

fn area(points: &[[f64; 2]]) -> f64 {
    let sum: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    (sum / 2.).abs()
}

/// Even-odd test for a point inside a closed loop.
fn inside(points: &[[f64; 2]], point: [f64; 2]) -> bool {
    let mut result = false;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}

/// Groups loops by containment. Even depth loops are outer boundaries.
/// Odd depth loops are holes in their nearest outer boundary.
fn nest(contours: Vec<Vec<[f64; 2]>>) -> Vec<SketchProfile> {
    let count = contours.len();
    let areas: Vec<f64> = contours.iter().map(|c| area(c)).collect();
    let parent: Vec<Option<usize>> = (0..count)
        .map(|i| {
            (0..count)
                .filter(|&j| j != i && areas[j] > areas[i] && inside(&contours[j], contours[i][0]))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        })
        .collect();
    let depth: Vec<usize> = (0..count)
        .map(|i| {
            let mut depth = 0;
            let mut link = parent[i];
            while let Some(j) = link {
                depth += 1;
                link = parent[j];
            }
            depth
        })
        .collect();
    (0..count)
        .filter(|&i| depth[i].is_multiple_of(2))
        .map(|i| SketchProfile {
            outer: polygon(&contours[i]),
            holes: (0..count)
                .filter(|&j| parent[j] == Some(i))
                .map(|j| polygon(&contours[j]))
                .collect(),
        })
        .collect()
}

fn polygon(points: &[[f64; 2]]) -> SketchLoop {
    SketchLoop::Polygon {
        points: points.to_vec(),
        corner_radius: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> SketchLoop {
        SketchLoop::Text {
            text: value.into(),
            height: 10.,
            at: [0., 0.],
            align: TextAlign::Left,
            angle_deg: None,
            font: None,
        }
    }

    fn outline(points: &SketchLoop) -> &[[f64; 2]] {
        match points {
            SketchLoop::Polygon { points, .. } => points,
            _ => panic!("expected a polygon"),
        }
    }

    #[test]
    fn letter_with_a_counter_has_one_hole() {
        let regions = expand(&text("O")).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1);
        assert!(area(outline(&regions[0].holes[0])) < area(outline(&regions[0].outer)));
    }

    #[test]
    fn eight_has_two_holes_and_i_has_a_dot() {
        let eight = expand(&text("8")).unwrap();
        assert_eq!((eight.len(), eight[0].holes.len()), (1, 2));
        assert_eq!(expand(&text("i")).unwrap().len(), 2);
    }

    #[test]
    fn capital_i_spans_the_cap_height() {
        let regions = expand(&text("I")).unwrap();
        let points = outline(&regions[0].outer);
        let low = points.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let high = points.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((high - low - 10.).abs() < 1e-6, "{low} {high}");
    }

    #[test]
    fn built_in_fonts_are_known_and_others_are_refused() {
        let mut hack = text("I");
        if let SketchLoop::Text { font, .. } = &mut hack {
            *font = Some("hack".into());
        }
        let regions = expand(&hack).unwrap();
        assert_eq!(regions.len(), 1);
        let mut unknown = text("I");
        if let SketchLoop::Text { font, .. } = &mut unknown {
            *font = Some("comic".into());
        }
        assert!(expand(&unknown).is_err());
    }

    #[test]
    fn extrude_refuses_a_non_text_open_surface() {
        // Two faces that do not come from a text sketch must not become one prism each.
        let kernel = crate::OcctKernel;
        let placement = Placement::default();
        let square = |x: f64| SketchProfile {
            outer: SketchLoop::Polygon {
                points: vec![[x, 0.], [x + 10., 0.], [x + 10., 10.], [x, 10.]],
                corner_radius: None,
            },
            holes: Vec::new(),
        };
        let left = kernel.sketch(&square(0.), &placement).unwrap();
        let right = kernel.sketch(&square(20.), &placement).unwrap();
        let faces = kernel.combine(&left, &right, Combine::Union).unwrap();
        assert_eq!(kernel.face_count(&faces).unwrap(), 2);
        assert!(kernel.extrude_shape(&faces, [0., 0., 1.]).is_err());
    }

    #[test]
    fn refuses_empty_multi_line_and_bad_height() {
        assert!(expand(&text("")).is_err());
        assert!(expand(&text("a\nb")).is_err());
        let zero = SketchLoop::Text {
            text: "I".into(),
            height: 0.,
            at: [0., 0.],
            align: TextAlign::Left,
            angle_deg: None,
            font: None,
        };
        assert!(expand(&zero).is_err());
    }
}
