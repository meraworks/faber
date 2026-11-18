use faber_ports::Result;
use faber_ports::vector::{Anchor, Path2, VectorDoc};
use std::fmt::Write;

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn render(doc: &VectorDoc) -> Result<String> {
    let [width, height] = doc.size_mm;
    let sheet_point = |[x, y]: [f64; 2]| -> Result<[f64; 2]> {
        let y = height - y;
        if x.is_finite() && y.is_finite() {
            Ok([x, y])
        } else {
            Err("The projected coordinates exceed the numeric range.".into())
        }
    };
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}mm" height="{height}mm" viewBox="0 0 {width} {height}">"#
    );
    for layer in &doc.layers {
        let [r, g, b] = layer.colour;
        let colour = format!("#{r:02x}{g:02x}{b:02x}");
        let dash = if layer.dashed {
            r#" stroke-dasharray="4 2""#
        } else {
            ""
        };
        writeln!(
            out,
            r#"<g data-layer="{}" fill="none" stroke="{colour}" stroke-width="{}"{dash}>"#,
            xml(&layer.name),
            layer.width_mm
        )
        .unwrap();
        for path in &layer.paths {
            match path {
                Path2::Polyline { points, closed } => {
                    let tag = if *closed { "polygon" } else { "polyline" };
                    write!(out, r#"<{tag} points=""#).unwrap();
                    for &point in points {
                        let [x, y] = sheet_point(point)?;
                        write!(out, "{x},{y} ").unwrap();
                    }
                    out.push_str("\"/>\n");
                }
                Path2::Circle { centre, radius } => {
                    let [x, y] = sheet_point(*centre)?;
                    writeln!(out, r#"<circle cx="{x}" cy="{y}" r="{radius}"/>"#).unwrap();
                }
                Path2::Arc {
                    centre: [x, y],
                    radius,
                    start_deg,
                    end_deg,
                } => {
                    let start = start_deg.rem_euclid(360.);
                    let end = end_deg.rem_euclid(360.);
                    let large = u8::from((end - start).rem_euclid(360.) > 180.);
                    let endpoint = |deg: f64| {
                        let (sin, cos) = deg.to_radians().sin_cos();
                        sheet_point([x + radius * cos, y + radius * sin])
                    };
                    let [x0, y0] = endpoint(start)?;
                    let [x1, y1] = endpoint(end)?;
                    writeln!(
                        out,
                        r#"<path d="M {x0} {y0} A {radius} {radius} 0 {large} 0 {x1} {y1}"/>"#
                    )
                    .unwrap();
                }
            }
        }
        for text in doc.texts.iter().filter(|text| text.layer == layer.name) {
            let [x, y] = sheet_point(text.at)?;
            let anchor = match text.anchor {
                Anchor::Start => "start",
                Anchor::Centre => "middle",
                Anchor::End => "end",
            };
            writeln!(out,r#"<text x="{x}" y="{y}" font-size="{}" text-anchor="{anchor}" stroke="none" fill="{colour}">{}</text>"#,text.height_mm,xml(&text.text)).unwrap();
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    Ok(out)
}
