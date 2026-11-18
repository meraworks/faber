//! Sheet coordinates in millimetres. The origin is at the lower left.

use crate::Result;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct VectorDoc {
    pub size_mm: [f64; 2],
    pub layers: Vec<Layer>,
    pub texts: Vec<Text>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub colour: [u8; 3],
    pub width_mm: f64,
    pub dashed: bool,
    pub paths: Vec<Path2>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Path2 {
    Polyline {
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
    /// Angles run counter-clockwise. For a full turn, use Circle.
    Arc {
        centre: [f64; 2],
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub at: [f64; 2],
    pub height_mm: f64,
    pub text: String,
    pub anchor: Anchor,
    pub layer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Centre,
    End,
}

pub trait VectorExport: Send + Sync {
    fn write(&self, doc: &VectorDoc, path: &Path) -> Result<()>;
}
