//! Loads and fixtures for a study of one part.
use crate::FaceSelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(bound(deserialize = "N: Deserialize<'de>"))]
pub struct Study<N = f64> {
    #[serde(flatten, deserialize_with = "read_kind")]
    pub kind: StudyKind<N>,
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

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StudyKind<N = f64> {
    #[default]
    Static,
    Modal {
        modes: u32,
    },
    Thermal {
        conditions: Vec<ThermalCondition<N>>,
        stress: bool,
        reference_c: N,
    },
    Shape {
        target_volume: N,
        keep: Vec<FaceSelector<N>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        min_member_mm: Option<N>,
        symmetry: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThermalCondition<N = f64> {
    Temperature {
        face: FaceSelector<N>,
        celsius: N,
    },
    Power {
        face: FaceSelector<N>,
        watts: N,
    },
    Convection {
        face: FaceSelector<N>,
        w_m2k: N,
        ambient_c: N,
    },
}

// Read the fields left after the common study data. Old studies have none.
fn read_kind<'de, N: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<StudyKind<N>, D::Error> {
    #[derive(Deserialize)]
    #[serde(
        tag = "kind",
        rename_all = "snake_case",
        deny_unknown_fields,
        bound(deserialize = "N: Deserialize<'de>")
    )]
    enum Current<N> {
        Static {},
        Modal {
            modes: u32,
        },
        Thermal {
            conditions: Vec<ThermalCondition<N>>,
            stress: bool,
            reference_c: N,
        },
        Shape {
            target_volume: N,
            #[serde(default)]
            keep: Vec<FaceSelector<N>>,
            min_member_mm: Option<N>,
            #[serde(default)]
            symmetry: Vec<String>,
        },
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    #[derive(Deserialize)]
    #[serde(untagged, bound(deserialize = "N: Deserialize<'de>"))]
    enum Input<N> {
        Current(Current<N>),
        Legacy(Empty),
    }
    Ok(match Input::deserialize(d)? {
        Input::Legacy(_) | Input::Current(Current::Static {}) => StudyKind::Static,
        Input::Current(Current::Modal { modes }) => StudyKind::Modal { modes },
        Input::Current(Current::Thermal {
            conditions,
            stress,
            reference_c,
        }) => StudyKind::Thermal {
            conditions,
            stress,
            reference_c,
        },
        Input::Current(Current::Shape {
            target_volume,
            keep,
            min_member_mm,
            symmetry,
        }) => StudyKind::Shape {
            target_volume,
            keep,
            min_member_mm,
            symmetry,
        },
    })
}
impl<'de, N: Deserialize<'de>> Deserialize<'de> for StudyKind<N> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_kind(d)
    }
}

impl<N> StudyKind<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<StudyKind<T>, String> {
        Ok(match self {
            Self::Static => StudyKind::Static,
            Self::Modal { modes } => StudyKind::Modal { modes: *modes },
            Self::Thermal {
                conditions,
                stress,
                reference_c,
            } => StudyKind::Thermal {
                conditions: conditions
                    .iter()
                    .map(|c| {
                        Ok(match c {
                            ThermalCondition::Temperature { face, celsius } => {
                                ThermalCondition::Temperature {
                                    face: face.map_numbers(map)?,
                                    celsius: map(celsius)?,
                                }
                            }
                            ThermalCondition::Power { face, watts } => ThermalCondition::Power {
                                face: face.map_numbers(map)?,
                                watts: map(watts)?,
                            },
                            ThermalCondition::Convection {
                                face,
                                w_m2k,
                                ambient_c,
                            } => ThermalCondition::Convection {
                                face: face.map_numbers(map)?,
                                w_m2k: map(w_m2k)?,
                                ambient_c: map(ambient_c)?,
                            },
                        })
                    })
                    .collect::<Result<_, String>>()?,
                stress: *stress,
                reference_c: map(reference_c)?,
            },
            Self::Shape {
                target_volume,
                keep,
                min_member_mm,
                symmetry,
            } => StudyKind::Shape {
                target_volume: map(target_volume)?,
                keep: keep
                    .iter()
                    .map(|f| f.map_numbers(map))
                    .collect::<Result<_, _>>()?,
                min_member_mm: min_member_mm.as_ref().map(map).transpose()?,
                symmetry: symmetry.clone(),
            },
        })
    }
}
