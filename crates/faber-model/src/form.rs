//! Editable control cages for subdivision forms.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SymmetryPlane {
    X,
    Y,
    Z,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Crease {
    pub edge: [u32; 2],
    pub sharpness: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CageData<N = f64> {
    pub vertices: Vec<[N; 3]>,
    pub faces: Vec<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creases: Vec<Crease>,
}
impl<N> CageData<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<CageData<T>, String> {
        Ok(CageData {
            vertices: self
                .vertices
                .iter()
                .map(|[x, y, z]| Ok::<_, String>([map(x)?, map(y)?, map(z)?]))
                .collect::<Result<_, _>>()?,
            faces: self.faces.clone(),
            creases: self.creases.clone(),
        })
    }
}
