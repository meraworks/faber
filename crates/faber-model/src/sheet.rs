//! A bent sheet uses one thickness and one bend rule.
use crate::SketchLoop;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct SheetRule<N = f64> {
    pub thickness: N,
    pub bend_radius: N,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k_factor: Option<N>,
    #[serde(default)]
    pub relief: Relief<N>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Relief<N = f64> {
    #[default]
    None,
    Rectangular {
        width: N,
        depth: N,
    },
    Round {
        diameter: N,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct Flange<N = f64> {
    pub id: String,
    pub on: String,
    pub edge: u32,
    pub length: N,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<N>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inset_start: Option<N>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inset_end: Option<N>,
    #[serde(default)]
    pub holes: Vec<SketchLoop<N>>,
}

impl<N> Relief<N> {
    pub fn map_numbers<T>(
        &self,
        f: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Relief<T>, String> {
        Ok(match self {
            Self::None => Relief::None,
            Self::Rectangular { width, depth } => Relief::Rectangular {
                width: f(width)?,
                depth: f(depth)?,
            },
            Self::Round { diameter } => Relief::Round {
                diameter: f(diameter)?,
            },
        })
    }
}
impl<N> SheetRule<N> {
    pub fn map_numbers<T>(
        &self,
        f: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SheetRule<T>, String> {
        Ok(SheetRule {
            thickness: f(&self.thickness)?,
            bend_radius: f(&self.bend_radius)?,
            k_factor: self.k_factor.as_ref().map(&mut *f).transpose()?,
            relief: self.relief.map_numbers(f)?,
        })
    }
}
impl<N> Flange<N> {
    pub fn map_numbers<T>(
        &self,
        f: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<Flange<T>, String> {
        Ok(Flange {
            id: self.id.clone(),
            on: self.on.clone(),
            edge: self.edge,
            length: f(&self.length)?,
            angle_deg: self.angle_deg.as_ref().map(&mut *f).transpose()?,
            inset_start: self.inset_start.as_ref().map(&mut *f).transpose()?,
            inset_end: self.inset_end.as_ref().map(&mut *f).transpose()?,
            holes: self
                .holes
                .iter()
                .map(|h| h.map_numbers(f))
                .collect::<Result<_, _>>()?,
        })
    }
}
