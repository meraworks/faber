//! Expand source helpers before the project reads the resulting form recipe.
use faber_form::{Cage, FormOp, apply, primitive_input::Primitive};
use faber_model::{Geometry, SymmetryPlane};
use faber_ports::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    op: String,
    shape: Primitive,
    levels: Option<u32>,
    #[serde(default)]
    as_solid: bool,
    #[serde(default)]
    symmetry: Vec<SymmetryPlane>,
    #[serde(default)]
    ops: Vec<FormOp>,
}

pub(crate) fn expand(geometry: &mut Value) -> Result<()> {
    match geometry.get("op").and_then(Value::as_str) {
        Some("history") => {
            if let Some(list) = geometry.get_mut("features").and_then(Value::as_array_mut) {
                for item in list {
                    if let Some(g) = item.get_mut("geometry") {
                        expand(g)?;
                    }
                }
            }
        }
        Some("form_primitive") => {
            let input: Input =
                serde_json::from_value(geometry.take()).map_err(|e| e.to_string())?;
            debug_assert_eq!(input.op, "form_primitive");
            let cage = Cage::from_data(&input.shape.cage()?)?;
            let cage = apply(&cage, &input.ops, &input.symmetry)?.to_data();
            *geometry = serde_json::to_value(Geometry::Form {
                cage,
                levels: input.levels,
                as_solid: input.as_solid,
                symmetry: input.symmetry,
            })
            .map_err(|e| e.to_string())?;
        }
        _ => (),
    }
    Ok(())
}
