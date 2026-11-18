mod common;
use common::*;
use faber_drawing::render;
use faber_model::drawing::{PartsList, Projection};
use serde_json::json;

#[test]
fn base_view_is_scaled_and_placed() {
    let mut v = base();
    v["scale"] = json!(2);
    v["hidden_lines"] = json!(true);
    let r = render(&drawing(json!({"front":v})), &Block(40.)).unwrap();
    assert_eq!(
        segment(&layer(&r.doc, "VISIBLE").paths[0]),
        [[60., 90.], [140., 90.]]
    );
    let h = layer(&r.doc, "HIDDEN");
    assert!(h.dashed);
    assert_eq!(h.paths.len(), 1);
}
#[test]
fn projection_angle_places_the_top_view_and_keeps_it_aligned() {
    for (p, y) in [(Projection::Third, 140.), (Projection::First, 60.)] {
        let mut d = drawing(
            json!({"front":base(),"top":{"kind":"projected","at":[137,140],"from":"front","side":"top"}}),
        );
        d.projection = p;
        let r = render(&d, &Block(40.)).unwrap();
        let edges = &layer(&r.doc, "VISIBLE").paths;
        assert!(
            edges
                .iter()
                .any(|e| segment(e) == [[80., y - 10.], [120., y - 10.]]),
            "{edges:?}"
        );
    }
}
#[test]
fn section_view_has_a_hatch_that_stops_at_the_hole() {
    let d = drawing(
        json!({"front":base(),"cut":{"kind":"section","at":[180,100],"from":"front","line":[[6,0],[6,10]]}}),
    );
    let r = render(&d, &Block(40.)).unwrap();
    let paths = &layer(&r.doc, "HATCH").paths;
    assert!(paths.len() > 5);
    let mut intercepts = Vec::new();
    for e in paths {
        let [a, b] = segment(e);
        assert!(((b[1] - a[1]) - (b[0] - a[0])).abs() < 1e-8);
        for k in 0..=10 {
            let t = k as f64 / 10.;
            let x = (a[0] * (1. - t) + b[0] * t - 180.).abs();
            let y = (a[1] * (1. - t) + b[1] * t - 100.).abs();
            assert!(x <= 10.00001 && y <= 10.00001 && (x >= 3.99999 || y >= 3.99999));
        }
        intercepts.push((a[1] - a[0]) / 2f64.sqrt());
    }
    intercepts.sort_by(f64::total_cmp);
    intercepts.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    assert!(
        intercepts
            .windows(2)
            .all(|w| ((w[1] - w[0]) - 3.).abs() < 1e-7)
    );
    assert!(r.doc.texts.iter().filter(|t| t.text == "A").count() >= 2);
}
#[test]
fn detail_clips_the_parent_before_enlargement() {
    let d = drawing(
        json!({"base":base(),"detail":{"kind":"detail","from":"base","at":[200,100],"scale":2,"centre":[20,0],"radius":3}}),
    );
    let r = render(&d, &Block(40.)).unwrap();
    assert!(layer(&r.doc, "VISIBLE").paths.iter().any(|p| matches!(
        p,
        faber_ports::vector::Path2::Polyline { .. }
    ) && segment(p)
        == [[194., 100.], [206., 100.]]));
}
#[test]
fn view_larger_than_the_sheet_is_reported() {
    let mut v = base();
    v["scale"] = json!(2);
    let r = render(&drawing(json!({"front":v})), &Block(200.)).unwrap();
    assert!(
        r.problems
            .contains(&"View front is 400 x 20 mm at scale 2. The sheet is 297 x 210 mm.".into()),
        "{:?}",
        r.problems
    );
}
#[test]
fn title_block_has_the_fields_and_grouped_parts() {
    let mut d = drawing(json!({"base":base()}));
    d.title_block.insert("Author".into(), "A&B".into());
    d.parts_list = Some(PartsList {
        at: [15., 60.],
        parts: vec![],
    });
    let r = render(&d, &Block(40.)).unwrap();
    let text = r
        .doc
        .texts
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for s in ["Bracket", "Author", "A&B", "Washer", "Steel", "2.5", "4"] {
        assert!(text.contains(s), "{text}");
    }
}

#[test]
fn circular_view_references_are_refused() {
    let d = drawing(
        json!({"a":{"kind":"projected","from":"b","side":"left","at":[0,0]},"b":{"kind":"projected","from":"a","side":"right","at":[0,0]}}),
    );
    assert!(render(&d, &Block(40.)).is_err());
}

#[test]
fn projection_symbol_shows_the_small_end_from_the_correct_side() {
    use faber_ports::vector::Path2;
    for projection in [Projection::First, Projection::Third] {
        let mut d = drawing(json!({}));
        d.projection = projection;
        let doc = render(&d, &Block(40.)).unwrap().doc;
        let paths = &layer(&doc, "FRAME").paths;
        let circle_x = paths
            .iter()
            .find_map(|p| {
                if let Path2::Circle { centre, .. } = p {
                    Some(centre[0])
                } else {
                    None
                }
            })
            .unwrap();
        let cone = paths
            .iter()
            .find_map(|p| {
                if let Path2::Polyline {
                    points,
                    closed: true,
                } = p
                    && points.len() == 4
                    && (points[0][1] - points[3][1]).abs() < (points[1][1] - points[2][1]).abs()
                {
                    Some(points)
                } else {
                    None
                }
            })
            .unwrap();
        let small_end_is_left = cone[0][0] < cone[1][0];
        let end_view_is_left = circle_x < cone[0][0];
        assert_eq!(
            small_end_is_left == end_view_is_left,
            projection == Projection::Third
        );
    }
}

#[test]
fn cutting_arrows_look_into_the_retained_material() {
    let d = drawing(
        json!({"front":base(),"cut":{"kind":"section","at":[180,100],"from":"front","line":[[6,0],[6,10]]}}),
    );
    let r = render(&d, &Block(40.)).unwrap();
    let arrow = segment(&layer(&r.doc, "CUTTING").paths[1]);
    // For the upward cutting line, x cross y points to the left.
    assert!(arrow[1][0] > arrow[0][0]);
}
