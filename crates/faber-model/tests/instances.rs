use faber_model::{Geometry, Scalar};
use serde_json::json;

#[test]
fn instance_refers_to_the_source_after_number_resolution() {
    let value = json!({"op":"instance","of":"shaft"});
    let recipe: Geometry<Scalar> = serde_json::from_value(value.clone()).unwrap();
    let resolved: Geometry<f64> = recipe
        .map_numbers(&mut |_| Err("An instance has no dimensions.".into()))
        .unwrap();
    assert_eq!(serde_json::to_value(resolved).unwrap(), value);
    assert!(
        serde_json::from_value::<Geometry<Scalar>>(
            json!({"op":"instance","of":"shaft","radius":3})
        )
        .is_err()
    );
}
