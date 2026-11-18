//! Printer dimensions and diametral fit allowances, in millimetres.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clearance {
    pub press: f64,
    pub snug: f64,
    pub slide: f64,
    pub loose: f64,
}

impl Default for Clearance {
    fn default() -> Self {
        // Practice values from the print preparation plan. Calibrate them with a fit sample.
        Self {
            press: 0.,
            snug: 0.15,
            slide: 0.25,
            loose: 0.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "Settings")]
pub struct Printer {
    pub name: String,
    pub bed_mm: [f64; 3],
    pub nozzle_mm: f64,
    pub layer_mm: f64,
    pub min_wall_mm: f64,
    pub overhang_deg: f64,
    pub bridge_mm: f64,
    pub clearance_mm: Clearance,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Settings {
    name: String,
    bed_mm: [f64; 3],
    nozzle_mm: f64,
    layer_mm: f64,
    min_wall_mm: Option<f64>,
    overhang_deg: Option<f64>,
    bridge_mm: Option<f64>,
    #[serde(default)]
    clearance_mm: Clearance,
}

impl Default for Printer {
    fn default() -> Self {
        Self {
            name: "Generic 220".into(),
            bed_mm: [220., 220., 250.],
            nozzle_mm: 0.4,
            layer_mm: 0.2,
            min_wall_mm: 0.8,
            overhang_deg: 45.,
            bridge_mm: 10.,
            clearance_mm: Clearance::default(),
        }
    }
}

impl TryFrom<Settings> for Printer {
    type Error = String;

    fn try_from(s: Settings) -> Result<Self, String> {
        let printer = Self {
            name: s.name,
            bed_mm: s.bed_mm,
            nozzle_mm: s.nozzle_mm,
            layer_mm: s.layer_mm,
            min_wall_mm: s.min_wall_mm.unwrap_or(2. * s.nozzle_mm),
            overhang_deg: s.overhang_deg.unwrap_or(45.),
            bridge_mm: s.bridge_mm.unwrap_or(10.),
            clearance_mm: s.clearance_mm,
        };
        printer.validate()?;
        Ok(printer)
    }
}

impl Printer {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Give the printer a name.".into());
        }
        if self
            .bed_mm
            .into_iter()
            .chain([
                self.nozzle_mm,
                self.layer_mm,
                self.min_wall_mm,
                self.bridge_mm,
            ])
            .any(|v| !v.is_finite() || v <= 0.)
        {
            return Err("Printer dimensions must be finite and greater than zero.".into());
        }
        if !(0. ..=90.).contains(&self.overhang_deg) {
            return Err("The overhang angle must be between 0 and 90 degrees.".into());
        }
        let c = self.clearance_mm;
        let values = [c.press, c.snug, c.slide, c.loose];
        if values.iter().any(|v| !v.is_finite() || *v < 0.) || !values.is_sorted() {
            return Err(
                "Fit allowances must be finite, non-negative and in ascending order.".into(),
            );
        }
        Ok(())
    }
}
