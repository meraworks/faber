//! Orthographic model coordinates. The observer is on the positive x cross y axis.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewFrame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection2 {
    pub visible: Vec<Curve2>,
    pub hidden: Vec<Curve2>,
    pub bounds: [[f64; 2]; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Curve2 {
    Line([[f64; 2]; 2]),
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
    /// Counter-clockwise in the drawing frame. The end can exceed 360 degrees.
    Arc {
        centre: [f64; 2],
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
    Polyline(Vec<[f64; 2]>),
}
