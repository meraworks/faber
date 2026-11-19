//! Convert named sketch outlines to lines, arcs and repeated holes.
pub mod rounded;
pub mod sketch_arrays;
pub mod slot;

fn positive(value: f64) -> Result<(), String> {
    if value.is_finite() && (1e-6..=1e6).contains(&value) {
        Ok(())
    } else {
        Err("Dimension must be between 0.000001 and 1000000 mm".into())
    }
}
