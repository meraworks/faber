#![allow(dead_code)]
use faber_drawing::{ModelSource, PartRow, Resolved};
use faber_model::drawing::{Anchor, Drawing};
use faber_ports::Result;
use faber_ports::drawing::{Curve2, Projection2, ViewFrame};
use faber_ports::vector::{Path2, VectorDoc};
use serde_json::{Value, json};

pub struct Block(pub f64);
pub fn sheet() -> Value {
    json!({"size":"a4","orientation":"landscape"})
}
pub fn base() -> Value {
    json!({"kind":"base","at":[100,100],"parts":[],"direction":"front"})
}
pub fn drawing(views: Value) -> Drawing {
    serde_json::from_value(
        json!({"title":"Bracket","sheet":sheet(),"projection":"third","scale":1,"views":views}),
    )
    .unwrap()
}
pub fn layer<'a>(doc: &'a VectorDoc, name: &str) -> &'a faber_ports::vector::Layer {
    doc.layers.iter().find(|l| l.name == name).unwrap()
}
pub fn segment(p: &Path2) -> [[f64; 2]; 2] {
    match p {
        Path2::Polyline { points, .. } => [points[0], *points.last().unwrap()],
        _ => panic!("Expected a line"),
    }
}
pub fn vertex(x: f64, y: f64, z: f64) -> Anchor {
    Anchor::Vertex {
        part: "bracket".into(),
        near: [x, y, z],
    }
}
pub fn hole(i: usize) -> Anchor {
    serde_json::from_value(json!({"kind":"cylinder_axis","part":"bracket","face":{"near_centre":[i as f64*6.,3.,0.],"tolerance":0.01,"expect":1}})).unwrap()
}
impl ModelSource for Block {
    fn bounds(&self, _: &[String]) -> Result<[[f64; 3]; 2]> {
        Ok([[0.; 3], [self.0, 20., 10.]])
    }
    fn project(&self, _: &[String], f: &ViewFrame, hidden: bool) -> Result<Projection2> {
        let extent = |a: [f64; 3]| self.0 * a[0].abs() + 20. * a[1].abs() + 10. * a[2].abs();
        let (w, h) = (extent(f.x), extent(f.y));
        let corners = [[0., 0.], [w, 0.], [w, h], [0., h]];
        Ok(Projection2 {
            visible: (0..4)
                .map(|i| Curve2::Line([corners[i], corners[(i + 1) % 4]]))
                .collect(),
            hidden: if hidden {
                vec![Curve2::Line([[0., h / 2.], [w, h / 2.]])]
            } else {
                vec![]
            },
            bounds: [[0., 0.], [w, h]],
        })
    }
    fn section(&self, _: &[String], _: [f64; 3], _: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>> {
        Ok(vec![
            vec![[-10., -10.], [10., -10.], [10., 10.], [-10., 10.]],
            vec![[-4., -4.], [4., -4.], [4., 4.], [-4., 4.]],
        ])
    }
    fn resolve(&self, a: &Anchor) -> Result<Resolved> {
        match a {
            Anchor::Vertex { near, .. } => {
                let mut p = *near;
                if p[0] == 40. {
                    p[0] = self.0
                }
                Ok(Resolved::Point(p))
            }
            Anchor::CylinderAxis { face, .. } => Ok(Resolved::Circle {
                centre: face.near_centre.unwrap(),
                axis: [0., 0., 1.],
                radius: 5.,
            }),
            Anchor::Edge { part, .. } if part == "edge_x" => {
                Ok(Resolved::Line([[0.; 3], [10., 0., 0.]]))
            }
            Anchor::Edge { part, .. } if part == "edge_z" => {
                Ok(Resolved::Line([[0.; 3], [0., 0., 10.]]))
            }
            _ => Err("The edge reference was not found.".into()),
        }
    }
    fn anchors(&self, _: &[String]) -> Result<Vec<(Anchor, Resolved)>> {
        let mut a = vec![vertex(0., 0., 0.), vertex(40., 20., 10.)];
        a.extend((0..4).map(hole));
        a.into_iter()
            .map(|a| Ok((a.clone(), self.resolve(&a)?)))
            .collect()
    }
    fn parts_list(&self, _: &[String]) -> Result<Vec<PartRow>> {
        Ok(vec![PartRow {
            quantity: 4,
            name: "Washer".into(),
            material: "Steel".into(),
            mass_g: Some(2.5),
        }])
    }
}
