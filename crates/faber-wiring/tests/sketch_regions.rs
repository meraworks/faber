use faber_model::{Part, Scalar};
use serde_json::{Value, json};
use std::f64::consts::PI;

fn outline(id: &str, plane: &str, circles: &[(&str, [f64; 2], f64)]) -> Value {
    let mut sketch = json!({"entities":{},"points":{}});
    for (name, pos, radius) in circles {
        let centre = format!("{name}_centre");
        sketch["points"][&centre] = json!(pos);
        sketch["entities"][*name] = json!({"kind":"circle","radius":radius,"centre":centre});
    }
    json!({"id":id,"geometry":{"op":"solved_sketch","plane":plane,"sketch":sketch}})
}
fn work(id: &str, z: f64) -> Value {
    json!({"id":id,"geometry":{"op":"plane","placement":{"translation":[0,0,z]}}})
}
fn build(mut features: Vec<Value>, op: Value) -> Result<f64, String> {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Regions".into())).unwrap();
    features.push(json!({"id":"made","geometry":op}));
    let part:Part<Scalar>=serde_json::from_value(json!({"name":"Regions","colour":[40,50,60],"geometry":{"op":"history","output":"made","features":features}})).unwrap();
    p.put_part("regions".into(), part)?;
    Ok(p.bodies[0].volume)
}
const ROW: [(&str, [f64; 2], f64); 2] = [("left", [5., 0.], 1.), ("right", [10., 0.], 2.)];
fn two() -> Vec<Value> {
    vec![work("base", 0.), outline("s", "base", &ROW)]
}
fn near(value: f64, expected: f64) {
    assert!(
        (value - expected).abs() < expected * 1e-7,
        "{value} != {expected}"
    );
}

#[test]
fn two_regions_extrude_together() {
    near(
        build(
            two(),
            json!({"op":"extrude_sketch","source":"s","vector":[0,0,4]}),
        )
        .unwrap(),
        20. * PI,
    );
}
#[test]
fn a_material_island_in_a_hole_keeps_all_three_contours() {
    let s = outline(
        "s",
        "base",
        &[
            ("inner", [0., 0.], 2.),
            ("hole", [0., 0.], 4.),
            ("outer", [0., 0.], 7.),
        ],
    );
    near(
        build(
            vec![work("base", 0.), s],
            json!({"op":"extrude_sketch","source":"s","vector":[0,0,2]}),
        )
        .unwrap(),
        74. * PI,
    );
}
#[test]
fn two_regions_revolve_around_the_same_axis() {
    near(
        build(
            two(),
            json!({"op":"revolve_sketch","source":"s","origin":[0,0,0],"axis":[0,1,0],"angle":360}),
        )
        .unwrap(),
        90. * PI * PI,
    );
}
#[test]
fn two_regions_sweep_the_same_spine() {
    let mut f = two();
    f.push(json!({"id":"track","geometry":{"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[0,0,4]}]}}}));
    near(
        build(f, json!({"op":"sweep_sketch","source":"s","path":"track"})).unwrap(),
        20. * PI,
    );
}
#[test]
fn two_regions_loft_by_position_when_feature_names_change_order() {
    let mut f = two();
    f.extend([
        work("upper", 4.),
        outline("t", "upper", &[("aa", [10., 0.], 2.), ("zz", [5., 0.], 1.)]),
    ]);
    near(
        build(
            f,
            json!({"op":"loft_sketches","sources":["s","t"],"ruled":true}),
        )
        .unwrap(),
        20. * PI,
    );
}
#[test]
fn a_loft_refuses_an_ambiguous_region_match() {
    let mut f = vec![
        work("base", 0.),
        outline("s", "base", &[("a", [-5., 0.], 1.), ("b", [5., 0.], 1.)]),
    ];
    f.extend([
        work("upper", 4.),
        outline("t", "upper", &[("a", [0., -5.], 1.), ("b", [0., 5.], 1.)]),
    ]);
    let err = build(f, json!({"op":"loft_sketches","sources":["s","t"]})).unwrap_err();
    assert!(err.contains("ambiguous"), "{err}");
}
