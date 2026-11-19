mod common;
use faber_model::{Part, Scalar};
use serde_json::{Value, json};

#[test]
fn a_partial_sketch_saves_until_a_solid_requires_its_profile() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Open sketch".into())).unwrap();
    let mut part = plate();
    let features = part["geometry"]["features"].as_array_mut().unwrap();
    features[1]["geometry"]["sketch"] = json!({"points":{"p":[0,0],"q":[3,4]},
        "entities":{"edge":{"kind":"line","a":"p","b":"q"}}});
    features[2]["geometry"] = json!({"op":"box","size":[2,2,2]});
    put(&mut p, part.clone()).unwrap();
    assert_eq!(p.sketch_warnings()["plate"]["outline"], 4);
    let before = common::project_files(dir.path());
    part["geometry"]["features"][2]["geometry"] =
        json!({"op":"extrude_sketch","source":"outline","vector":[0,0,4]});
    let err = put(&mut p, part).unwrap_err();
    assert!(
        err.contains("outline") && err.contains("open ends are at p and q"),
        "{err}"
    );
    assert_eq!(common::project_files(dir.path()), before);
}

fn plate() -> Value {
    json!({"name":"Plate","colour":[90,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"work","geometry":{"op":"plane","placement":{"translation":[0,0,7]}}},
        {"id":"outline","geometry":{"op":"solved_sketch","plane":"work","sketch":{
            "points":{"a":[0,0],"b":[40,0],"c":[40,20],"d":[0,20]},
            "entities":{"ab":{"kind":"line","a":"a","b":"b"},"bc":{"kind":"line","a":"b","b":"c"},
              "cd":{"kind":"line","a":"c","b":"d"},"da":{"kind":"line","a":"d","b":"a"}},
            "constraints":[{"kind":"fix","point":"a"},{"kind":"horizontal","line":"ab"},
              {"kind":"horizontal","line":"cd"},{"kind":"vertical","line":"bc"},{"kind":"vertical","line":"da"},
              {"kind":"distance_x","a":"a","b":"b","value":40},{"kind":"distance_y","a":"a","b":"d","value":20}]
        }}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"outline","vector":[0,0,10]}}
    ]}})
}

fn put(p: &mut faber_core::Project, value: Value) -> Result<(), String> {
    p.put_part(
        "plate".into(),
        serde_json::from_value::<Part<Scalar>>(value).unwrap(),
    )
}

#[test]
fn dimension_from_a_parameter_rebuilds_a_placed_extrusion() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Plate".into())).unwrap();
    let mut part = plate();
    part["geometry"]["features"][1]["geometry"]["sketch"]["constraints"][5]["value"] = json!("w");
    p.set_params([("w".into(), "40".into())].into()).unwrap();
    put(&mut p, part).unwrap();
    assert!((p.bodies[0].volume - 8000.).abs() < 1e-6);
    assert!((p.bodies[0].bounds[0][2] - 7.).abs() < 1e-6);
    p.set_params([("w".into(), "50".into())].into()).unwrap();
    assert!((p.bodies[0].volume - 10000.).abs() < 1e-6);
}

#[test]
fn over_constrained_sketch_is_refused_and_files_stay() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Plate".into())).unwrap();
    let mut part = plate();
    put(&mut p, part.clone()).unwrap();
    let before = common::project_files(dir.path());
    part["geometry"]["features"][1]["geometry"]["sketch"]["constraints"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"distance_x","a":"a","b":"b","value":50}));
    let error = put(&mut p, part).unwrap_err();
    assert!(
        error.contains("outline") && error.to_lowercase().contains("conflict"),
        "{error}"
    );
    assert_eq!(common::project_files(dir.path()), before);
}

#[test]
fn under_constrained_sketch_warns_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Plate".into())).unwrap();
    let mut part = plate();
    part["geometry"]["features"][1]["geometry"]["sketch"]["constraints"]
        .as_array_mut()
        .unwrap()
        .pop();
    put(&mut p, part).unwrap();
    drop(p);
    let p = faber_wiring::open(dir.path(), false, None).unwrap();
    let warnings = p.sketch_warnings();
    assert_eq!(warnings["plate"]["outline"], 1);
    let mut p = p;
    put(
        &mut p,
        json!({"name":"Block","colour":[100,150,180],"geometry":{"op":"box","size":[4,4,4]}}),
    )
    .unwrap();
    assert!(p.sketch_warnings().is_empty());
}

#[test]
fn offset_sketch_of_a_rectangle() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Plate".into())).unwrap();
    let mut part = plate();
    let features = part["geometry"]["features"].as_array_mut().unwrap();
    features.extend([
        json!({"id":"inside","geometry":{"op":"offset_sketch","sketch":"outline","distance":-2}}),
        json!({"id":"plug","geometry":{"op":"extrude_sketch","source":"inside","vector":[0,0,10]}}),
        json!({"id":"rim","geometry":{"op":"difference","left":"pad","right":"plug"}}),
    ]);
    part["geometry"]["output"] = json!("rim");
    put(&mut p, part).unwrap();
    assert!((p.bodies[0].volume - 2240.).abs() < 1e-6);
}

#[test]
fn inset_with_two_holes_shrinks_the_outline_and_grows_the_holes() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Two holes".into())).unwrap();
    let mut part = plate();
    let features = part["geometry"]["features"].as_array_mut().unwrap();
    let sketch = &mut features[1]["geometry"]["sketch"];
    for (id, x) in [("left", 10), ("right", 30)] {
        let name = format!("{id}_centre");
        sketch["points"][&name] = json!([x, 10]);
        sketch["entities"][id] = json!({"kind":"circle","centre":name,"radius":2});
    }
    features.insert(
        2,
        json!({"id":"inset","geometry":{"op":"offset_sketch","sketch":"outline","distance":-1}}),
    );
    features[3]["geometry"]["source"] = json!("inset");
    put(&mut p, part).unwrap();
    let volume = (38. * 18. - 18. * std::f64::consts::PI) * 10.;
    assert!(
        (p.bodies[0].volume - volume).abs() < 1e-6,
        "{}",
        p.bodies[0].volume
    );
}
