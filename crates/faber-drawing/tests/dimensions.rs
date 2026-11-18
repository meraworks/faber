mod common;
use common::*;
use faber_drawing::{auto_drawing, render};
use serde_json::json;
fn linear() -> faber_model::drawing::Drawing {
    let mut d = drawing(json!({"front":base()}));
    d.dimensions.insert("d3".into(),serde_json::from_value(json!({"kind":"linear","view":"front","offset":8,"along":"horizontal","a":vertex(0.,0.,0.),"b":vertex(40.,0.,0.)})).unwrap());
    d
}
#[test]
fn linear_dimension_follows_the_model_and_can_override_the_text() {
    let mut d = linear();
    for width in [40., 50.] {
        let r = render(&d, &Block(width)).unwrap();
        assert!(r.doc.texts.iter().any(|t| t.text == width.to_string()));
        assert!(layer(&r.doc, "DIMENSION").paths.len() >= 5);
    }
    d.dimensions.get_mut("d3").unwrap().text = Some("Overall".into());
    assert!(
        render(&d, &Block(40.))
            .unwrap()
            .doc
            .texts
            .iter()
            .any(|t| t.text == "Overall")
    );
}
#[test]
fn lost_reference_is_reported() {
    let mut d = linear();
    d.dimensions.insert("d3".into(),serde_json::from_value(json!({"kind":"radius","view":"front","offset":4,"edge":{"kind":"edge","part":"gone","edge":{"tolerance":0.01,"expect":1}}})).unwrap());
    let r = render(&d, &Block(40.)).unwrap();
    assert!(
        r.problems
            .contains(&"Dimension d3: the edge reference was not found.".into())
    );
    let t = r.doc.texts.iter().find(|t| t.text == "?").unwrap();
    let c = layer(&r.doc, &t.layer).colour;
    assert!(c[0] > c[1] && c[0] > c[2]);
}
#[test]
fn four_equal_holes_are_one_measured_diameter_and_eight_locations() {
    let m = Block(40.);
    let d = auto_drawing(&m, &[], serde_json::from_value(sheet()).unwrap(), "Bracket").unwrap();
    let r = render(&d, &m).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let dia: Vec<_> = r
        .doc
        .texts
        .iter()
        .filter(|t| t.text.contains('Ø') || t.text.contains('⌀'))
        .collect();
    assert_eq!(dia.len(), 1);
    assert!(
        dia[0].text.starts_with("4x") && dia[0].text.ends_with("10"),
        "{dia:?}"
    );
    assert!(d.dimensions.len() >= 12);
}
#[test]
fn auto_drawing_fits_the_sheet() {
    let m = Block(300.);
    let d = auto_drawing(&m, &[], serde_json::from_value(sheet()).unwrap(), "Bracket").unwrap();
    assert_eq!(d.scale, 0.5);
    assert_eq!(d.views.len(), 4);
    assert!(render(&d, &m).unwrap().problems.is_empty());
}
#[test]
fn angle_is_measured_between_the_projected_edges() {
    let mut d = linear();
    let edge = |part: &str| json!({"kind":"edge","part":part,"edge":{"tolerance":0.01,"expect":1}});
    d.dimensions.insert(
        "angle".into(),
        serde_json::from_value(
            json!({"kind":"angle","view":"front","offset":8,"a":edge("edge_x"),"b":edge("edge_z")}),
        )
        .unwrap(),
    );
    let r = render(&d, &Block(40.)).unwrap();
    assert!(r.doc.texts.iter().any(|t| t.text == "90°"));
    assert!(
        layer(&r.doc, "DIMENSION")
            .paths
            .iter()
            .any(|p| matches!(p, faber_ports::vector::Path2::Arc { .. }))
    );
}

#[test]
fn aligned_hole_coordinates_have_separate_labels() {
    let m = Block(40.);
    let d = auto_drawing(&m, &[], serde_json::from_value(sheet()).unwrap(), "Bracket").unwrap();
    let doc = render(&d, &m).unwrap().doc;
    let positions: Vec<_> = doc
        .texts
        .iter()
        .filter(|t| t.layer == "DIMENSION" && t.text == "3")
        .map(|t| t.at)
        .collect();
    assert_eq!(positions.len(), 4);
    for (i, a) in positions.iter().enumerate() {
        for b in &positions[i + 1..] {
            assert!(
                (a[0] - b[0]).hypot(a[1] - b[1]) >= 4.,
                "Overlapping labels at {a:?} and {b:?}"
            );
        }
    }
}

#[test]
fn equal_bores_on_different_axes_include_the_isometric_view() {
    use faber_drawing::{ModelSource, Resolved};
    use faber_model::drawing::{Anchor, DimensionKind};
    use faber_ports::{
        Result,
        drawing::{Projection2, ViewFrame},
    };
    struct Bores(Block);
    impl ModelSource for Bores {
        fn bounds(&self, p: &[String]) -> Result<[[f64; 3]; 2]> {
            self.0.bounds(p)
        }
        fn project(&self, p: &[String], f: &ViewFrame, h: bool) -> Result<Projection2> {
            self.0.project(p, f, h)
        }
        fn section(&self, p: &[String], o: [f64; 3], n: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>> {
            self.0.section(p, o, n)
        }
        fn resolve(&self, a: &Anchor) -> Result<Resolved> {
            let mut value = self.0.resolve(a)?;
            if let Resolved::Circle { centre, axis, .. } = &mut value {
                *axis = if centre[0] == 0. {
                    [1., -1., 1.]
                } else {
                    [1., 0., 0.]
                };
            }
            Ok(value)
        }
        fn anchors(&self, p: &[String]) -> Result<Vec<(Anchor, Resolved)>> {
            self.0
                .anchors(p)?
                .into_iter()
                .map(|(a, _)| Ok((a.clone(), self.resolve(&a)?)))
                .collect()
        }
    }
    let m = Bores(Block(40.));
    let d = auto_drawing(&m, &[], serde_json::from_value(sheet()).unwrap(), "Bores").unwrap();
    assert_eq!(
        d.dimensions
            .values()
            .filter(|d| d.view == "iso" && matches!(d.kind, DimensionKind::Linear { .. }))
            .count(),
        2
    );
    assert_eq!(
        d.dimensions
            .values()
            .filter(|d| matches!(
                serde_json::to_value(&d.kind).unwrap()["kind"].as_str(),
                Some("diameter" | "diameter_group")
            ))
            .count(),
        1
    );
    let r = render(&d, &m).unwrap();
    assert!(r.doc.texts.iter().any(|t| t.text == "4x Ø10"));
}

#[test]
fn changed_or_removed_group_member_cannot_keep_an_old_diameter_count() {
    use faber_drawing::{ModelSource, Resolved};
    use faber_model::drawing::Anchor;
    use faber_ports::{
        Result,
        drawing::{Projection2, ViewFrame},
    };
    struct Changed {
        removed: bool,
    }
    impl ModelSource for Changed {
        fn project(&self, p: &[String], f: &ViewFrame, h: bool) -> Result<Projection2> {
            Block(40.).project(p, f, h)
        }
        fn section(&self, p: &[String], o: [f64; 3], n: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>> {
            Block(40.).section(p, o, n)
        }
        fn bounds(&self, p: &[String]) -> Result<[[f64; 3]; 2]> {
            Block(40.).bounds(p)
        }
        fn resolve(&self, a: &Anchor) -> Result<Resolved> {
            let mut value = Block(40.).resolve(a)?;
            if let Resolved::Circle { centre, radius, .. } = &mut value
                && centre[0] == 12.
            {
                if self.removed {
                    return Err("The cylindrical face reference was not found.".into());
                }
                *radius = 6.;
            }
            Ok(value)
        }
    }
    let mut d = auto_drawing(&Block(40.), &[], Default::default(), "Bores").unwrap();
    d.dimensions.retain(|k, _| k.starts_with("diameter"));
    for removed in [false, true] {
        let r = render(&d, &Changed { removed }).unwrap();
        assert!(
            r.problems
                .iter()
                .any(|msg| msg.starts_with("Dimension diameter1:"))
        );
        assert!(
            r.doc
                .texts
                .iter()
                .any(|t| t.layer == "BROKEN" && t.text == "?")
        );
        assert!(!r.doc.texts.iter().any(|t| t.text.starts_with("4x")));
    }
}

#[test]
fn grouped_diameter_text_clears_the_outline_and_other_labels() {
    use glam::DVec2;
    let m = Block(40.);
    let d = auto_drawing(&m, &[], Default::default(), "Bores").unwrap();
    let r = render(&d, &m).unwrap();
    let t = r
        .doc
        .texts
        .iter()
        .find(|t| t.text.starts_with("4x"))
        .unwrap();
    let centre = DVec2::from(d.views["top"].at);
    let span = DVec2::new(40., 20.) * d.scale;
    let text = DVec2::from(t.at);
    let half = DVec2::new(t.text.chars().count() as f64, 2.);
    assert!(
        ((text - centre).abs() - span / 2. - half).max_element() >= 1.,
        "The leader text touches the outline"
    );
    for other in r
        .doc
        .texts
        .iter()
        .filter(|u| u.layer == "DIMENSION" && u.text != t.text)
    {
        let other_half = DVec2::new(other.text.chars().count() as f64, 2.);
        assert!(((text - DVec2::from(other.at)).abs() - half - other_half).max_element() >= 0.5);
    }
}
