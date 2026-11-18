use super::{Geometry, Scalar};
use serde_json::{Value, from_value, json, to_value};

fn selector() -> Value {
    json!({"created_by":"base","normal":[0.,0.,1.],"near_centre":null,"in_box":null,"area":null,
           "tolerance":0.01,"expect":1})
}

#[test]
fn face_operations_round_trip_and_reject_unknown_fields() {
    let face = selector();
    for operation in [
        json!({"op":"remove_faces","body":"base","faces":[face]}),
        json!({"op":"move_faces","body":"base","faces":[face],"distance":"travel"}),
        json!({"op":"split_face","body":"base","face":face,"tool":{"kind":"plane","plane":"mid"}}),
        json!({"op":"split_face","body":"base","face":face,"tool":{"kind":"sketch","sketch":"outline"}}),
        json!({"op":"emboss","body":"base","face":face,"sketch":"letters","depth":"depth","mode":"raise"}),
        json!({"op":"emboss","body":"base","face":face,"sketch":"letters","depth":2.,"mode":"engrave","at":[4.,"height"]}),
    ] {
        let geometry: Geometry<Scalar> = from_value(operation.clone()).unwrap();
        assert_eq!(to_value(geometry).unwrap(), operation);
        let mut extra = operation;
        extra["unused"] = json!(true);
        assert!(from_value::<Geometry<Scalar>>(extra).is_err());
    }
    for tool in [
        json!({"kind":"plane","plane":"mid","sketch":"outline"}),
        json!({"kind":"text","sketch":"label"}),
    ] {
        assert!(
            from_value::<Geometry<Scalar>>(
                json!({"op":"split_face","body":"b","face":face,"tool":tool})
            )
            .is_err()
        );
    }
}

#[test]
fn face_operations_resolve_selectors_distances_depth_and_wrapped_origin() {
    let mut face = selector();
    face["area"] = json!("area");
    for operation in [
        json!({"op":"remove_faces","body":"base","faces":[face]}),
        json!({"op":"move_faces","body":"base","faces":[face],"distance":"travel"}),
        json!({"op":"split_face","body":"base","face":face,"tool":{"kind":"sketch","sketch":"path"}}),
        json!({"op":"emboss","body":"base","face":face,"sketch":"outline","depth":"depth","mode":"raise","at":["arc","height"]}),
    ] {
        let geometry: Geometry<Scalar> = from_value(operation.clone()).unwrap();
        let mapped = geometry
            .map_numbers(&mut |x| match x {
                Scalar::Number(n) => Ok(*n),
                Scalar::Expression(s) => match s.as_str() {
                    "area" => Ok(50.),
                    "travel" => Ok(-3.),
                    "arc" => Ok(8.),
                    "height" => Ok(12.),
                    "depth" => Ok(1.5),
                    name => Err(format!("Unknown parameter: {name}.")),
                },
            })
            .unwrap();
        let v = to_value(mapped).unwrap();
        assert_eq!(v["body"], "base");
        let picked = if v.get("faces").is_some() {
            &v["faces"][0]
        } else {
            &v["face"]
        };
        assert_eq!(picked["created_by"], "base");
        assert_eq!(picked["area"], 50.);
        match v["op"].as_str().unwrap() {
            "move_faces" => assert_eq!(v["distance"], -3.),
            "emboss" => {
                assert_eq!(v["depth"], 1.5);
                assert_eq!(v["at"], json!([8., 12.]));
            }
            "split_face" => assert_eq!(v["tool"], operation["tool"]),
            _ => {}
        }
    }
}
