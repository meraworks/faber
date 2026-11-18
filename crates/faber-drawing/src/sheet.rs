use crate::{ModelSource, format_length, path};
use faber_model::drawing::{Drawing, Projection};
use faber_ports::{
    Result,
    vector::{Anchor, Layer, Path2, Text, VectorDoc},
};
use glam::DVec2;

pub(crate) fn add(doc: &mut VectorDoc, layer: &str, path: Path2) {
    doc.layers
        .iter_mut()
        .find(|l| l.name == layer)
        .unwrap()
        .paths
        .push(path);
}
pub(crate) fn line(doc: &mut VectorDoc, l: &str, a: DVec2, b: DVec2) {
    add(doc, l, path([a, b], false));
}
pub(crate) fn text(doc: &mut VectorDoc, layer: &str, at: DVec2, text: String, height: f64) {
    doc.texts.push(Text {
        at: at.to_array(),
        height_mm: height,
        text,
        anchor: Anchor::Centre,
        layer: layer.into(),
    });
}
fn entry(doc: &mut VectorDoc, at: [f64; 2], text: String) {
    doc.texts.push(Text {
        at,
        height_mm: 2.8,
        text,
        anchor: Anchor::Start,
        layer: "TEXT".into(),
    });
}
pub(crate) fn arrow(doc: &mut VectorDoc, l: &str, tip: DVec2, direction: DVec2) {
    let d = direction.normalize_or_zero();
    let side = d.perp() * 0.6;
    let tail = tip - d * 2.8;
    add(doc, l, path([tail + side, tip, tail - side], false));
}
pub(crate) fn cut_line(doc: &mut VectorDoc, a: DVec2, b: DVec2, label: &str) {
    let dir = -(b - a).normalize_or_zero().perp();
    line(doc, "CUTTING", a, b);
    for p in [a, b] {
        line(doc, "CUTTING", p - dir * 6., p);
        arrow(doc, "CUTTING", p, dir);
        text(doc, "TEXT", p - dir * 9., label.into(), 3.);
    }
}
fn rect(doc: &mut VectorDoc, a: DVec2, b: DVec2) {
    add(
        doc,
        "FRAME",
        path([a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)], true),
    );
}
pub(crate) fn title_bounds(d: &Drawing) -> (DVec2, DVec2) {
    let w = d.sheet.size_mm()[0];
    (
        DVec2::new(w - 97., 5.),
        DVec2::new(w - 5., 20. + 5. * d.title_block.len() as f64),
    )
}
pub(crate) fn sheet(d: &Drawing, model: &dyn ModelSource) -> Result<VectorDoc> {
    let size = d.sheet.size_mm();
    let [w, h] = size;
    let mut doc = VectorDoc {
        size_mm: size,
        texts: vec![],
        layers: [
            ("FRAME", 0.35, false),
            ("HIDDEN", 0.25, true),
            ("VISIBLE", 0.5, false),
            ("HATCH", 0.18, false),
            ("SECTION", 0.7, false),
            ("CUTTING", 0.35, true),
            ("DIMENSION", 0.25, false),
            ("TEXT", 0.25, false),
            ("BROKEN", 0.3, false),
        ]
        .into_iter()
        .map(|(name, width_mm, dashed)| Layer {
            name: name.into(),
            width_mm,
            dashed,
            colour: if name == "BROKEN" {
                [190, 32, 28]
            } else {
                [34, 37, 40]
            },
            paths: vec![],
        })
        .collect(),
    };
    rect(&mut doc, DVec2::splat(5.), DVec2::new(w - 5., h - 5.));
    let (a, b) = title_bounds(d);
    let left = a.x;
    let head = b.y;
    rect(&mut doc, a, b);
    entry(&mut doc, [left + 2., head - 4.5], d.title.clone());
    line(
        &mut doc,
        "FRAME",
        DVec2::new(left, head - 6.),
        DVec2::new(w - 5., head - 6.),
    );
    for (i, (key, value)) in d.title_block.iter().enumerate() {
        entry(
            &mut doc,
            [left + 2., head - 10.5 - i as f64 * 5.],
            format!("{key}: {value}"),
        );
    }
    let ratio = if d.scale >= 1. {
        format!("{}:1", format_length(d.scale))
    } else {
        format!("1:{}", format_length(1. / d.scale))
    };
    entry(&mut doc, [left + 2., 8.], format!("Scale {ratio}"));
    let first = d.projection == Projection::First;
    entry(
        &mut doc,
        [left + 32., 8.],
        if first { "First angle" } else { "Third angle" }.into(),
    );
    let x = w - 24.;
    let y = 10.;
    let cone = x + if first { -4. } else { 6. };
    let ring = x + if first { 11. } else { -1. };
    add(
        &mut doc,
        "FRAME",
        path(
            [
                [cone - 3., y - 2.],
                [cone + 3., y - 3.],
                [cone + 3., y + 3.],
                [cone - 3., y + 2.],
            ]
            .map(DVec2::from),
            true,
        ),
    );
    for radius in [2., 3.] {
        add(
            &mut doc,
            "FRAME",
            Path2::Circle {
                centre: [ring, y],
                radius,
            },
        );
    }
    for note in &d.notes {
        text(
            &mut doc,
            "TEXT",
            note.at.into(),
            note.text.clone(),
            note.height_mm.unwrap_or(3.),
        );
    }
    if let Some(list) = &d.parts_list {
        let [x, y] = list.at;
        let rows = model.parts_list(&list.parts)?;
        for (i, columns) in std::iter::once([
            "Item".into(),
            "Qty".into(),
            "Name".into(),
            "Material".into(),
            "Mass (g)".into(),
        ])
        .chain(rows.into_iter().enumerate().map(|(i, r)| {
            [
                (i + 1).to_string(),
                r.quantity.to_string(),
                r.name,
                r.material,
                r.mass_g.map(format_length).unwrap_or_else(|| "—".into()),
            ]
        }))
        .enumerate()
        {
            let yy = y - i as f64 * 5.;
            for (c, dx) in columns.into_iter().zip([0., 10., 20., 56., 84.]) {
                entry(&mut doc, [x + dx, yy], c);
            }
            line(
                &mut doc,
                "FRAME",
                DVec2::new(x, yy - 1.4),
                DVec2::new(x + 102., yy - 1.4),
            );
        }
    }
    Ok(doc)
}
