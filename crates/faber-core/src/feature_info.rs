//! Reads one feature of a part. The read does not change the project.

use crate::{Project, history, placement};
use faber_model::{Feature, Scalar};
use faber_ports::Result;

/// One feature. The kind is the operation name, such as `box` or `plane`.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureInfo {
    pub kind: &'static str,
    pub reading: Reading,
}

/// What a feature gives in part coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// A solid. A suppressed modifier gives the solid that it passes on.
    Solid {
        volume_mm3: f64,
        bounds_mm: [[f64; 3]; 2],
        faces: u32,
        edges: u32,
        suppressed: bool,
        depends_on: Vec<String>,
    },
    /// A suppressed creation feature. It gives no solid.
    Suppressed,
    Plane {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        normal: [f64; 3],
    },
    Axis {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Point {
        point: [f64; 3],
    },
}

/// The feature whose solid a feature gives. A suppressed feature passes on its primary input.
/// None when the chain ends without a solid.
fn solid_source<'a>(features: &'a [Feature<Scalar>], feature: &'a str) -> Option<&'a str> {
    let mut current = feature;
    loop {
        let entry = features.iter().find(|entry| entry.id == current)?;
        if !entry.suppressed {
            return Some(current);
        }
        current = history::primary_input(&entry.geometry)?;
    }
}

impl Project {
    /// Reads one feature of a part. Solids give their size. Reference features give their frame.
    pub fn feature_info(&self, id: &str, feature: &str) -> Result<FeatureInfo> {
        let part = self
            .data
            .parts
            .get(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        let faber_model::Geometry::History { features, .. } = &part.geometry else {
            return Err(format!("Part {id} has no feature history"));
        };
        let entry = features
            .iter()
            .find(|entry| entry.id == feature)
            .ok_or_else(|| format!("Unknown feature {feature} in part {id}"))?;
        let cache = self.histories.get(id);
        let reading = if let Some(frame) = cache.and_then(|cache| cache.placement(feature)) {
            let rotation = placement::rotation_matrix(frame.rotation);
            Reading::Plane {
                origin: frame.translation,
                x: rotation.col(0).to_array(),
                y: rotation.col(1).to_array(),
                normal: rotation.col(2).to_array(),
            }
        } else if let Some((origin, direction)) = cache.and_then(|cache| cache.axis(feature)) {
            Reading::Axis { origin, direction }
        } else if let Some(point) = cache.and_then(|cache| cache.point(feature)) {
            Reading::Point { point }
        } else {
            let source = solid_source(features, feature)
                .and_then(|source| cache.and_then(|cache| cache.shape(source)));
            match source {
                Some(shape) => {
                    let properties = self.kernel.properties(shape)?;
                    Reading::Solid {
                        volume_mm3: properties.volume,
                        bounds_mm: properties.bounds,
                        faces: properties.faces,
                        edges: properties.edges,
                        suppressed: entry.suppressed,
                        depends_on: history::references(&entry.geometry)
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    }
                }
                None if entry.suppressed => Reading::Suppressed,
                None => {
                    return Err(format!("Feature {feature} has no solid in part {id}"));
                }
            }
        };
        Ok(FeatureInfo {
            kind: history::operation_kind(&entry.geometry),
            reading,
        })
    }
}
