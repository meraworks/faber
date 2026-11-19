use faber_ports::Result;
use faber_ports::vector::{Anchor, Path2, VectorDoc};
use std::collections::BTreeSet;
use std::fmt::{Display, Write};

struct Dxf(String);

impl Dxf {
    fn group(&mut self, code: u16, value: impl Display) {
        writeln!(self.0, "{code}\n{value}").unwrap();
    }

    fn point(&mut self, code: u16, [x, y]: [f64; 2]) {
        self.group(code, x);
        self.group(code + 10, y);
        self.group(code + 20, 0);
    }

    fn start(&mut self, section: &str) {
        self.group(0, "SECTION");
        self.group(2, section);
    }

    fn table(&mut self, name: &str, count: usize) {
        self.group(0, "TABLE");
        self.group(2, name);
        self.group(70, count);
    }

    fn linetype(&mut self, name: &str, pattern: &[f64]) {
        self.group(0, "LTYPE");
        self.group(2, name);
        self.group(70, 0);
        self.group(3, name);
        self.group(72, 65);
        self.group(73, pattern.len());
        self.group(40, pattern.iter().map(|x| x.abs()).sum::<f64>());
        for length in pattern {
            self.group(49, length);
        }
    }

    fn layer(&mut self, name: &str, colour: u8, dashed: bool) {
        self.group(0, "LAYER");
        self.group(2, name);
        self.group(70, 0);
        self.group(62, colour);
        self.group(6, if dashed { "DASHED" } else { "CONTINUOUS" });
    }

    fn entity(&mut self, kind: &str, layer: &str) {
        self.group(0, kind);
        self.group(8, layer);
    }

    fn line(&mut self, layer: &str, a: [f64; 2], b: [f64; 2]) {
        self.entity("LINE", layer);
        self.point(10, a);
        self.point(11, b);
    }
}

// ACI 7 changes with the background. Use it for black or white.
fn colour(rgb: [u8; 3]) -> u8 {
    if rgb == [0; 3] || rgb == [255; 3] {
        return 7;
    }
    let palette = [
        (1, [255, 0, 0]),
        (2, [255, 255, 0]),
        (3, [0, 255, 0]),
        (4, [0, 255, 255]),
        (5, [0, 0, 255]),
        (6, [255, 0, 255]),
        (7, [255, 255, 255]),
        (250, [51, 51, 51]),
        (251, [80, 80, 80]),
        (252, [105, 105, 105]),
        (253, [130, 130, 130]),
        (254, [190, 190, 190]),
    ];
    palette
        .iter()
        .min_by_key(|(_, colour)| {
            rgb.iter()
                .zip(colour)
                .map(|(a, b)| (i32::from(*a) - b).pow(2))
                .sum::<i32>()
        })
        .unwrap()
        .0
}

fn ascii(s: &str) -> String {
    let mut out = String::new();
    let percent_codes = s.contains("%%");
    for (offset, ch) in s.char_indices() {
        match ch {
            '^' => out.push_str("^ "),
            '%' if percent_codes => out.push_str("%%%"),
            '\\' if s[offset..].starts_with(r"\U+") => out.push_str(r"\U+005C"),
            c if c.is_ascii() => out.push(c),
            c => {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    write!(out, r"\U+{unit:04X}").unwrap();
                }
            }
        }
    }
    out
}

pub(super) fn render(doc: &VectorDoc, join_closed: bool) -> Result<String> {
    let mut out = Dxf(String::new());
    out.start("HEADER");
    out.group(9, "$ACADVER");
    out.group(1, "AC1009");
    for (name, [x, y]) in [("$LIMMIN", [0., 0.]), ("$LIMMAX", doc.size_mm)] {
        out.group(9, name);
        out.group(10, x);
        out.group(20, y);
    }
    out.group(0, "ENDSEC");

    out.start("TABLES");
    out.table("LTYPE", 2);
    out.linetype("CONTINUOUS", &[]);
    out.linetype("DASHED", &[4., -2.]);
    out.group(0, "ENDTAB");

    let add_zero = !doc.layers.iter().any(|layer| layer.name == "0");
    let count = doc.layers.len() + usize::from(add_zero);
    if count > i16::MAX as usize {
        return Err(format!(
            "The drawing needs {count} layers. The R12 limit is 32767."
        ));
    }
    out.table("LAYER", count);
    if add_zero {
        out.layer("0", 7, false);
    }
    let mut names = BTreeSet::new();
    for layer in &doc.layers {
        if layer.name.len() > 31
            || !layer
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"$-_".contains(&c))
        {
            return Err(format!(
                "Invalid R12 layer name: {}. Use up to 31 letters, digits, $, - or _.",
                layer.name
            ));
        }
        if !names.insert(layer.name.to_ascii_uppercase()) {
            return Err(format!(
                "R12 layer names must differ regardless of case: {}.",
                layer.name
            ));
        }
        out.layer(&layer.name, colour(layer.colour), layer.dashed);
    }
    out.group(0, "ENDTAB");
    out.group(0, "ENDSEC");
    out.start("BLOCKS");
    out.group(0, "ENDSEC");

    out.start("ENTITIES");
    for layer in &doc.layers {
        for path in &layer.paths {
            match path {
                Path2::Polyline {
                    points,
                    closed: true,
                } if join_closed => {
                    out.entity("POLYLINE", &layer.name);
                    out.group(66, 1);
                    out.point(10, [0.; 2]);
                    out.group(70, 1);
                    let unique = if points.first() == points.last() {
                        &points[..points.len() - 1]
                    } else {
                        points.as_slice()
                    };
                    for &p in unique {
                        out.entity("VERTEX", &layer.name);
                        out.point(10, p);
                    }
                    out.entity("SEQEND", &layer.name);
                }
                Path2::Polyline { points, closed } => {
                    for pair in points.windows(2) {
                        out.line(&layer.name, pair[0], pair[1]);
                    }
                    if *closed && points.first() != points.last() {
                        out.line(&layer.name, *points.last().unwrap(), points[0]);
                    }
                }
                Path2::Circle { centre, radius } => {
                    out.entity("CIRCLE", &layer.name);
                    out.point(10, *centre);
                    out.group(40, radius);
                }
                Path2::Arc {
                    centre,
                    radius,
                    start_deg,
                    end_deg,
                } => {
                    out.entity("ARC", &layer.name);
                    out.point(10, *centre);
                    out.group(40, radius);
                    out.group(50, start_deg.rem_euclid(360.));
                    out.group(51, end_deg.rem_euclid(360.));
                }
            }
        }
    }
    for label in &doc.texts {
        let value = ascii(&label.text);
        if value.len() > 255 {
            return Err("A text label exceeds the R12 limit of 255 bytes.".into());
        }
        out.entity("TEXT", &label.layer);
        out.point(10, label.at);
        out.group(40, label.height_mm);
        out.group(1, value);
        let align = match label.anchor {
            Anchor::Start => 0,
            Anchor::Centre => 1,
            Anchor::End => 2,
        };
        if align != 0 {
            out.group(72, align);
            out.point(11, label.at);
        }
    }
    out.group(0, "ENDSEC");
    out.group(0, "EOF");
    Ok(out.0)
}
