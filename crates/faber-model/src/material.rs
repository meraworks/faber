use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialClass {
    Metal,
    Polymer,
}

/// Typical properties at room temperature. Strength and modulus use MPa.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaterialData {
    /// Maximum design strain for a short snap deflection, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap_strain_limit: Option<f64>,
    pub name: String,
    pub class: MaterialClass,
    pub density_g_cm3: f64,
    pub youngs_mpa: f64,
    pub poisson: f64,
    pub yield_mpa: f64,
    pub ultimate_mpa: f64,
    pub conductivity_w_mk: f64,
    pub expansion_um_mk: f64,
    pub specific_heat_j_kgk: f64,
    pub layer_strength_factor: f64,
    pub colour: [u8; 3],
    pub roughness: f64,
    pub metallic: f64,
    pub source: String,
}
