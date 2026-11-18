mod common;
use common::*;
use faber_drawing::{ModelSource, Resolved, auto_drawing, render};
use faber_model::drawing::{Anchor, Direction};
use faber_ports::{
    Result,
    drawing::{Curve2, Projection2, ViewFrame},
};
use glam::{DVec2, DVec3};

/// Each extreme is in the middle of the silhouette on its other axis.
struct Diamond(f64);
impl Diamond {
    fn points(&self) -> [[f64; 3]; 6] {
        let r = self.0 / 2.;
        [
            [-r, 0., 0.],
            [r, 0., 0.],
            [0., -10., 0.],
            [0., 10., 0.],
            [0., 0., -5.],
            [0., 0., 5.],
        ]
    }
    fn xy(&self, frame: &ViewFrame) -> Vec<DVec2> {
        self.points()
            .into_iter()
            .map(|p| {
                let p = DVec3::from(p);
                DVec2::new(p.dot(frame.x.into()), p.dot(frame.y.into()))
            })
            .collect()
    }
}
impl ModelSource for Diamond {
    fn bounds(&self, _: &[String]) -> Result<[[f64; 3]; 2]> {
        Ok([[-self.0 / 2., -10., -5.], [self.0 / 2., 10., 5.]])
    }
    fn resolve(&self, a: &Anchor) -> Result<Resolved> {
        let Anchor::Vertex { near, .. } = a else {
            return Err("Missing edge.".into());
        };
        self.points()
            .into_iter()
            .min_by(|a, b| {
                DVec3::from(*a)
                    .distance_squared((*near).into())
                    .total_cmp(&DVec3::from(*b).distance_squared((*near).into()))
            })
            .map(Resolved::Point)
            .ok_or("Missing vertex.".into())
    }
    fn anchors(&self, _: &[String]) -> Result<Vec<(Anchor, Resolved)>> {
        Ok(self
            .points()
            .map(|p| (vertex(p[0], p[1], p[2]), Resolved::Point(p)))
            .to_vec())
    }
    fn section(&self, _: &[String], _: [f64; 3], _: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>> {
        Ok(vec![])
    }
    fn project(&self, _: &[String], f: &ViewFrame, _: bool) -> Result<Projection2> {
        let points = self.xy(f);
        let lo = points.iter().copied().reduce(DVec2::min).unwrap();
        let hi = points.iter().copied().reduce(DVec2::max).unwrap();
        let outline = [[lo.x, 0.], [0., lo.y], [hi.x, 0.], [0., hi.y]];
        Ok(Projection2 {
            visible: (0..4)
                .map(|i| Curve2::Line([outline[i], outline[(i + 1) % 4]]))
                .collect(),
            hidden: vec![],
            bounds: [lo.into(), hi.into()],
        })
    }
}

#[test]
fn automatic_overall_lines_and_text_clear_each_silhouette() {
    let model = Diamond(40.);
    let drawing = auto_drawing(&model, &[], Default::default(), "Diamond").unwrap();
    for (id, dir, along) in [
        ("width", Direction::Front, 0),
        ("height", Direction::Front, 1),
        ("depth", Direction::Top, 1),
    ] {
        let mut d = drawing.clone();
        d.dimensions.retain(|k, _| k == id);
        let dim = &d.dimensions[id];
        let centre = d.views[&dim.view].at;
        let cross = 1 - along;
        let geometry = model
            .project(&[], &faber_drawing::frame(&dir), false)
            .unwrap();
        let half = (geometry.bounds[1][cross] - geometry.bounds[0][cross]) * d.scale * 0.5;
        let doc = render(&d, &model).unwrap().doc;
        let lines = &layer(&doc, "DIMENSION").paths;
        let measured = segment(&lines[2]);
        let label = doc.texts.iter().find(|t| t.layer == "DIMENSION").unwrap();
        let char_margin = if cross == 0 {
            label.text.len() as f64 * 0.95
        } else {
            1.5
        };
        assert!(
            (measured[0][cross] - centre[cross]).abs() >= half + 4.,
            "{id} crosses the silhouette"
        );
        assert!(
            (label.at[cross] - centre[cross]).abs() >= half + char_margin + 1.,
            "{id} label overlaps the silhouette"
        );
        // Each extension starts on the vertex that supplies the measured extreme.
        for line in &lines[..2] {
            assert!((segment(line)[0][cross] - centre[cross]).abs() < 1e-7);
        }
    }
}
#[test]
fn overall_width_follows_large_edits_when_nearest_vertices_change() {
    let mut d = auto_drawing(&Diamond(40.), &[], Default::default(), "Diamond").unwrap();
    d.dimensions.retain(|k, _| k == "width");
    let r = render(&d, &Diamond(100.)).unwrap();
    assert!(r.problems.is_empty());
    assert!(
        r.doc
            .texts
            .iter()
            .any(|t| t.layer == "DIMENSION" && t.text == "100")
    );
}

#[test]
fn auto_scale_fits_the_actual_projections_of_a_sphere() {
    struct Sphere;
    impl ModelSource for Sphere {
        fn bounds(&self, _: &[String]) -> Result<[[f64; 3]; 2]> {
            Ok([[-170.; 3], [170.; 3]])
        }
        fn resolve(&self, _: &Anchor) -> Result<Resolved> {
            Err("The sphere has no vertices.".into())
        }
        fn section(&self, _: &[String], _: [f64; 3], _: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>> {
            Ok(vec![])
        }
        fn project(&self, _: &[String], _: &ViewFrame, _: bool) -> Result<Projection2> {
            Ok(Projection2 {
                visible: vec![Curve2::Circle {
                    centre: [0.; 2],
                    radius: 170.,
                }],
                hidden: vec![],
                bounds: [[-170.; 2], [170.; 2]],
            })
        }
    }
    let d = auto_drawing(&Sphere, &[], Default::default(), "Sphere").unwrap();
    // Four 68 mm circles and the title fit on A4 at 1:5. Four 170 mm circles do not.
    assert_eq!(d.scale, 0.2);
    let r = render(&d, &Sphere).unwrap();
    assert!(r.problems.is_empty());
    for v in d.views.values() {
        assert!(v.at[0] - 34. >= 5. && v.at[0] + 34. <= 292.);
        assert!(v.at[1] - 34. >= 5. && v.at[1] + 34. <= 205.);
        assert!(v.at[0] + 34. < 200. || v.at[1] - 34. > 20.);
    }
    for t in r.doc.texts.iter().filter(|t| t.layer == "DIMENSION") {
        assert!((8.0..289.).contains(&t.at[0]) && (7.0..203.).contains(&t.at[1]));
        assert!(t.at[0] < 196. || t.at[1] > 22.);
    }
}
