use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RibSide {
    Symmetric,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LipLoop {
    Outer,
    Inner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LipSide {
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BossRibs<N = f64> {
    pub count: N,
    pub thickness: N,
    pub length: N,
    pub height: N,
}

impl<N> BossRibs<N> {
    pub(crate) fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<BossRibs<T>, String> {
        Ok(BossRibs {
            count: map(&self.count)?,
            thickness: map(&self.thickness)?,
            length: map(&self.length)?,
            height: map(&self.height)?,
        })
    }
}

pub(crate) fn map_lines<N, T>(
    lines: &[[[N; 2]; 2]],
    map: &mut impl FnMut(&N) -> Result<T, String>,
) -> Result<Vec<[[T; 2]; 2]>, String> {
    lines
        .iter()
        .map(|[[ax, ay], [bx, by]]| Ok([[map(ax)?, map(ay)?], [map(bx)?, map(by)?]]))
        .collect()
}
