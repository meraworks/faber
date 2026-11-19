//! References for operations on surfaces.
use crate::EdgeSelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeRef<N = f64> {
    Edge {
        feature: String,
        edge: Box<EdgeSelector<N>>,
    },
    Curve {
        feature: String,
    },
}
impl<N> EdgeRef<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<EdgeRef<T>, String> {
        Ok(match self {
            Self::Edge { feature, edge } => EdgeRef::Edge {
                feature: feature.clone(),
                edge: Box::new(edge.map_numbers(map)?),
            },
            Self::Curve { feature } => EdgeRef::Curve {
                feature: feature.clone(),
            },
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrimKeep<N = f64> {
    pub near: [N; 3],
}
impl<N> TrimKeep<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<TrimKeep<T>, String> {
        let [x, y, z] = &self.near;
        Ok(TrimKeep {
            near: [map(x)?, map(y)?, map(z)?],
        })
    }
}
