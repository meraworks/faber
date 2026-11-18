use faber_model::Geometry;
use serde_json::json;

#[test]
fn linked_and_unlinked_imports_round_trip_through_number_mapping() {
    let plain = json!({"op":"imported","asset":"0123"});
    let mut linked = plain.clone();
    linked["link"] = json!({"path":"board.step","hash":"0123"});
    for input in [plain, linked] {
        let geometry: Geometry = serde_json::from_value(input.clone()).unwrap();
        let mapped = geometry.map_numbers(&mut |x| Ok(*x)).unwrap();
        assert_eq!(serde_json::to_value(mapped).unwrap(), input);
    }
}
