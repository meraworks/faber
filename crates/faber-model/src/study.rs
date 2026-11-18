//! Loads and fixtures for a study of one part.
use crate::FaceSelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct Study<N = f64> {
    pub part: String,
    pub fixtures: Vec<Fixture<N>>,
    pub loads: Vec<StudyLoad<N>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_mm: Option<N>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub print_direction: Option<[N; 3]>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fixture<N = f64> {
    pub face: FaceSelector<N>,
    pub kind: FixtureKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FixtureKind {
    Fixed,
    Slide,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StudyLoad<N = f64> {
    Force {
        face: FaceSelector<N>,
        newtons: [N; 3],
    },
    Pressure {
        face: FaceSelector<N>,
        mpa: N,
    },
    Gravity {
        direction: [N; 3],
    },
}
