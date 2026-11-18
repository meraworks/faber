//! One-page paper output. The content stream uses sheet millimetres.
use super::paper;
use faber_ports::{
    Result,
    vector::{Text, VectorDoc},
};
use std::{fmt::Write as _, io::Write as _};
use tiny_skia::{Path, PathSegment, Point};

const POINTS_PER_MM: f64 = 72. / 25.4;
const EXTRA: &str = "€\0‚ƒ„…†‡ˆ‰Š‹Œ\0Ž\0\0‘’“”•–—˜™š›œ\0žŸ";

fn win_ansi(c: char) -> Option<u8> {
    if c.is_ascii() || (160..=255).contains(&(c as u32)) {
        return Some(c as u8);
    }
    EXTRA.chars().position(|v| v == c).map(|i| 128 + i as u8)
}

fn write_path(out: &mut String, path: &Path) {
    let mut cursor = Point::from_xy(0., 0.);
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                writeln!(out, "{:.5} {:.5} m", p.x, p.y).unwrap();
                cursor = p;
            }
            PathSegment::LineTo(p) => {
                writeln!(out, "{:.5} {:.5} l", p.x, p.y).unwrap();
                cursor = p;
            }
            PathSegment::CubicTo(a, b, p) => {
                writeln!(
                    out,
                    "{:.5} {:.5} {:.5} {:.5} {:.5} {:.5} c",
                    a.x, a.y, b.x, b.y, p.x, p.y
                )
                .unwrap();
                cursor = p;
            }
            PathSegment::QuadTo(q, p) => {
                let a = cursor + (q - cursor) * Point::from_xy(2. / 3., 2. / 3.);
                let b = p + (q - p) * Point::from_xy(2. / 3., 2. / 3.);
                writeln!(
                    out,
                    "{:.5} {:.5} {:.5} {:.5} {:.5} {:.5} c",
                    a.x, a.y, b.x, b.y, p.x, p.y
                )
                .unwrap();
                cursor = p;
            }
            PathSegment::Close => out.push_str("h\n"),
        }
    }
}

fn write_text(out: &mut String, label: &Text) -> Result<()> {
    if let Some(bytes) = label.text.chars().map(win_ansi).collect::<Option<Vec<_>>>() {
        let size = label.height_mm / 0.718;
        let width = bytes
            .iter()
            .map(|b| u64::from(WIDTHS[*b as usize]))
            .sum::<u64>() as f64
            * size
            / 1000.;
        let x = label.at[0] + paper::anchor_offset(label.anchor, width);
        write!(out, "BT /F1 {size:.5} Tf {x:.5} {:.5} Td (", label.at[1]).unwrap();
        for b in bytes {
            match b {
                b'(' | b')' | b'\\' => {
                    out.push('\\');
                    out.push(char::from(b));
                }
                32..=126 => out.push(char::from(b)),
                _ => write!(out, "\\{b:03o}").unwrap(),
            }
        }
        out.push_str(") Tj ET\n");
    } else if let Some(path) = paper::outline(label)? {
        out.push_str("/Span << /ActualText <FEFF");
        for unit in label.text.encode_utf16() {
            write!(out, "{unit:04X}").unwrap();
        }
        out.push_str("> >> BDC\n");
        // A text operator lets a reader copy the letters drawn by the path.
        let start = label.at[0];
        writeln!(
            out,
            "q BT /F1 {:.5} Tf 3 Tr {start:.5} {:.5} Td (x) Tj ET Q",
            label.height_mm, label.at[1]
        )
        .unwrap();
        write_path(out, &path);
        out.push_str("f\nEMC\n");
    }
    Ok(())
}

pub(super) fn render(doc: &VectorDoc) -> Result<Vec<u8>> {
    let [w, h] = doc.size_mm.map(|mm| mm * POINTS_PER_MM);
    if !w.is_finite() || !h.is_finite() {
        return Err("The paper size is outside the supported range.".into());
    }
    let mut stream = format!("q\n{POINTS_PER_MM:.8} 0 0 {POINTS_PER_MM:.8} 0 0 cm\n");
    for layer in &doc.layers {
        let [r, g, b] = layer.colour.map(|v| f64::from(v) / 255.);
        let dash = if layer.dashed { "[4 2]" } else { "[]" };
        writeln!(
            stream,
            "{r:.4} {g:.4} {b:.4} RG {r:.4} {g:.4} {b:.4} rg {:.5} w {dash} 0 d",
            layer.width_mm
        )
        .unwrap();
        for shape in &layer.paths {
            write_path(&mut stream, &paper::path(shape)?);
            stream.push_str("S\n");
        }
        for label in doc.texts.iter().filter(|l| l.layer == layer.name) {
            write_text(&mut stream, label)?;
        }
    }
    stream.push_str("Q\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut file = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(file.len());
        writeln!(file, "{} 0 obj\n{obj}\nendobj", i + 1).unwrap();
    }
    let xref = file.len();
    writeln!(file, "xref\n0 6\n0000000000 65535 f ").unwrap();
    for offset in offsets {
        writeln!(file, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(
        file,
        "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    )
    .unwrap();
    Ok(file)
}

// Helvetica advance widths in thousandths of an em, in WinAnsi order.
const WIDTHS: [u16; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
    0, 556, 1000, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 1000, 611, 1000, 1000,
    222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 1000, 500, 667, 278, 333, 556,
    556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333, 333, 333, 556,
    537, 278, 333, 333, 365, 556, 834, 834, 834, 611, 667, 667, 667, 667, 667, 667, 1000, 722, 667,
    667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722,
    722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278,
    278, 556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,
];
