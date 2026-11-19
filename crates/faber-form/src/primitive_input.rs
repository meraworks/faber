use crate::{try_form_box, try_form_cylinder, try_form_sphere, try_form_torus};
use faber_model::CageData;
use faber_ports::Result;
use serde::Deserialize;

fn side() -> [f64; 3] {
    [20.; 3]
}
fn one_each() -> [u32; 3] {
    [1; 3]
}
fn ten() -> f64 {
    10.
}
fn height() -> f64 {
    20.
}
fn minor() -> f64 {
    3.
}
fn one() -> u32 {
    1
}
fn four() -> u32 {
    4
}
fn sixteen() -> u32 {
    16
}
fn eight() -> u32 {
    8
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "primitive", rename_all = "snake_case", deny_unknown_fields)]
pub enum Primitive {
    Box {
        #[serde(default = "side")]
        size: [f64; 3],
        #[serde(default = "one_each")]
        segments: [u32; 3],
    },
    Cylinder {
        #[serde(default = "ten")]
        radius: f64,
        #[serde(default = "height")]
        height: f64,
        #[serde(default = "sixteen")]
        sides: u32,
        #[serde(default = "one")]
        segments: u32,
    },
    Sphere {
        #[serde(default = "ten")]
        radius: f64,
        #[serde(default = "four")]
        segments: u32,
    },
    Torus {
        #[serde(default = "ten")]
        major: f64,
        #[serde(default = "minor")]
        minor: f64,
        #[serde(default = "sixteen")]
        sides: u32,
        #[serde(default = "eight")]
        rings: u32,
    },
}
impl Primitive {
    pub fn cage(&self) -> Result<CageData> {
        match *self {
            Self::Box { size, segments } => try_form_box(size, segments),
            Self::Cylinder {
                radius,
                height,
                sides,
                segments,
            } => try_form_cylinder(radius, height, sides, segments),
            Self::Sphere { radius, segments } => try_form_sphere(radius, segments),
            Self::Torus {
                major,
                minor,
                sides,
                rings,
            } => try_form_torus(major, minor, sides, rings),
        }
    }
}
