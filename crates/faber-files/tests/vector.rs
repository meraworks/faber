use faber_files::VectorWriter;
use faber_ports::vector::{Anchor, Layer, Path2, Text, VectorDoc, VectorExport};

fn sheet() -> VectorDoc {
    VectorDoc {
        size_mm: [297., 210.],
        layers: vec![Layer {
            name: "BEND".into(),
            colour: [255, 0, 0],
            width_mm: 0.25,
            dashed: true,
            paths: vec![
                Path2::Polyline {
                    points: vec![[10., 20.], [15., 20.], [10., 25.]],
                    closed: true,
                },
                Path2::Circle {
                    centre: [10., 20.],
                    radius: 5.,
                },
                Path2::Arc {
                    centre: [0., 0.],
                    radius: 12.,
                    start_deg: 0.,
                    end_deg: 90.,
                },
            ],
        }],
        texts: vec![Text {
            at: [30., 40.],
            height_mm: 3.,
            text: "a<b&c".into(),
            anchor: Anchor::Centre,
            layer: "BEND".into(),
        }],
    }
}

fn output(ext: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("sheet.{ext}"));
    VectorWriter.write(&sheet(), &path).unwrap();
    std::fs::read_to_string(path).unwrap()
}

// Read the group codes in a record. Code zero starts the next record.
fn records<'a>(file: &'a str, ty: &str) -> Vec<Vec<(u16, &'a str)>> {
    let mut lines = file.lines();
    let mut records = Vec::new();
    let mut keep = false;
    while let Some(code) = lines.next() {
        let code = code.trim().parse::<u16>().unwrap();
        let value = lines.next().unwrap();
        if code == 0 {
            keep = value == ty;
            if keep {
                records.push(Vec::new());
            }
        } else if keep {
            records.last_mut().unwrap().push((code, value));
        }
    }
    records
}

fn group<'a>(record: &[(u16, &'a str)], key: u16) -> &'a str {
    record.iter().find(|(code, _)| *code == key).unwrap().1
}

#[test]
fn svg_has_the_sheet_size() {
    use quick_xml::{Reader, events::Event};
    let file = output("svg");
    let mut reader = Reader::from_str(&file);
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                assert_eq!(e.name().as_ref(), b"svg");
                for (key, expected) in [
                    ("width", "297mm"),
                    ("height", "210mm"),
                    ("viewBox", "0 0 297 210"),
                ] {
                    assert_eq!(
                        e.try_get_attribute(key).unwrap().unwrap().value.as_ref(),
                        expected.as_bytes()
                    );
                }
                break;
            }
            Event::Eof => panic!("Missing sheet"),
            _ => (),
        }
    }
}

#[test]
fn svg_flips_y() {
    let file = output("svg");
    let doc = roxmltree::Document::parse(&file).unwrap();
    let circle = doc
        .descendants()
        .find(|n| n.has_tag_name("circle"))
        .unwrap();
    assert_eq!(circle.attribute("cx"), Some("10"));
    assert_eq!(circle.attribute("cy"), Some("190"));
}

#[test]
fn dxf_circle_is_one_entity() {
    let file = output("dxf");
    let circles = records(&file, "CIRCLE");
    assert_eq!(circles.len(), 1);
    assert_eq!(group(&circles[0], 40).parse::<f64>().unwrap(), 5.);
    assert_eq!(group(&circles[0], 8), "BEND");
}

#[test]
fn dxf_arc_is_counter_clockwise() {
    let file = output("dxf");
    let arcs = records(&file, "ARC");
    assert_eq!(arcs.len(), 1);
    assert_eq!(group(&arcs[0], 50).parse::<f64>().unwrap(), 0.);
    assert_eq!(group(&arcs[0], 51).parse::<f64>().unwrap(), 90.);
}

#[test]
fn dashed_layer_is_dashed() {
    let file = output("svg");
    let svg = roxmltree::Document::parse(&file).unwrap();
    assert!(
        svg.descendants()
            .any(|n| n.attribute("stroke-dasharray").is_some())
    );
    let file = output("dxf");
    let layers = records(&file, "LAYER");
    let bend = layers.iter().find(|r| group(r, 2) == "BEND").unwrap();
    assert_eq!(group(bend, 6), "DASHED");
    assert!(
        records(&file, "LTYPE")
            .iter()
            .any(|r| group(r, 2) == "DASHED")
    );
}

#[test]
fn text_is_escaped() {
    let file = output("svg");
    let svg = roxmltree::Document::parse(&file).unwrap();
    let text = svg.descendants().find(|n| n.has_tag_name("text")).unwrap();
    assert_eq!(text.text(), Some("a<b&c"));
    assert_eq!(text.attribute("text-anchor"), Some("middle"));
}

#[test]
fn dxf_closes_the_polyline_and_places_text_on_its_layer() {
    let file = output("dxf");
    let lines = records(&file, "LINE");
    assert_eq!(lines.len(), 3);
    assert_eq!(group(&lines[2], 11).parse::<f64>().unwrap(), 10.);
    assert_eq!(group(&lines[2], 21).parse::<f64>().unwrap(), 20.);
    let labels = records(&file, "TEXT");
    assert_eq!(group(&labels[0], 1), "a<b&c");
    assert_eq!(group(&labels[0], 8), "BEND");
    assert_eq!(group(&labels[0], 72), "1");
}

#[test]
fn unknown_format_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sheet.jpg");
    let error = VectorWriter
        .write(&sheet(), &path)
        .unwrap_err()
        .to_lowercase();
    assert!(error.contains("svg") && error.contains("dxf"));
    assert!(!path.exists());
}

#[test]
fn invalid_document_preserves_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["svg", "dxf"] {
        let path = dir.path().join(format!("sheet.{ext}"));
        std::fs::write(&path, "previous").unwrap();
        let mut doc = sheet();
        doc.layers[0].paths.push(Path2::Circle {
            centre: [0., 0.],
            radius: f64::NAN,
        });
        assert!(VectorWriter.write(&doc, &path).is_err());
        let mut doc = sheet();
        doc.texts[0].layer = "missing".into();
        assert!(VectorWriter.write(&doc, &path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
    }
}

#[test]
fn svg_refuses_coordinates_that_overflow_after_projection() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("overflow.svg");
    std::fs::write(&output, "old drawing").unwrap();
    let mut doc = sheet();
    doc.size_mm = [10., f64::MAX];
    doc.layers[0].paths.clear();
    doc.texts.clear();
    for path in [
        Path2::Polyline {
            points: vec![[0., -f64::MAX], [1., 0.]],
            closed: false,
        },
        Path2::Circle {
            centre: [0., -f64::MAX],
            radius: 1.,
        },
        Path2::Arc {
            centre: [f64::MAX, 0.],
            radius: f64::MAX,
            start_deg: 0.,
            end_deg: 90.,
        },
    ] {
        doc.layers[0].paths = vec![path];
        assert!(VectorWriter.write(&doc, &output).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "old drawing");
    }
    doc.layers[0].paths.clear();
    doc.texts = sheet().texts;
    doc.texts[0].at[1] = -f64::MAX;
    assert!(VectorWriter.write(&doc, &output).is_err());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "old drawing");
}

#[test]
fn dxf_refuses_more_layers_than_the_table_can_count() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("layers.dxf");
    let mut doc = sheet();
    doc.texts.clear();
    let mut layer = doc.layers.pop().unwrap();
    layer.paths.clear();
    for i in 0..i16::MAX {
        layer.name = format!("L{i}");
        doc.layers.push(layer.clone());
    }
    let error = VectorWriter.write(&doc, &output).unwrap_err();
    assert!(error.contains("32768") && error.contains("32767"));
    assert!(!output.exists());
}

#[test]
fn dxf_protects_text_that_matches_control_codes() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("label.dxf");
    let mut doc = sheet();
    doc.texts[0].text = r"angle %%d / C^2 / literal \U+0041".into();
    VectorWriter.write(&doc, &destination).unwrap();
    let file = std::fs::read_to_string(destination).unwrap();
    let labels = records(&file, "TEXT");
    assert_eq!(
        group(&labels[0], 1),
        r"angle %%%%%%d / C^ 2 / literal \U+005CU+0041"
    );
}
