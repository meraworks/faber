use faber_model::{Part, Scalar};
use serde_json::{Value, json};
use std::f64::consts::PI;

/// Extrudes one sketch by 10 mm and returns the body volume, or the refusal text.
fn extruded_volume(profile: Value, params: &[(&str, &str)]) -> Result<f64, String> {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Rounded".into())).unwrap();
    project
        .set_params(
            params
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        )
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Plate","colour":[100,140,170],"geometry":{"op":"history","output":"pad","features":[
        {"id":"section","geometry":{"op":"sketch","profile":profile}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"section","vector":[0,0,10]}}
    ]}}))
    .unwrap();
    project
        .put_part("plate".into(), part)
        .map_err(|error| error.to_string())?;
    Ok(project.bodies[0].volume)
}

fn rounded_rectangle(radius: Value) -> Value {
    json!({"kind":"rounded_rectangle","centre":[0,0],"width":40,"height":20,"radius":radius,"rotation":0})
}

#[test]
fn rounded_rectangle_extrudes_to_its_exact_area() {
    // Four corners remove r^2 (4 - pi) in total. 40 x 20 with r 5 gives (800 - 25 (4 - pi)) mm2.
    let expected = (800. - 25. * (4. - PI)) * 10.;
    let volume = extruded_volume(json!({"outer": rounded_rectangle(json!(5))}), &[]).unwrap();
    assert!((volume - expected).abs() < 1e-6, "{volume}");
    // Rotation turns the same shape. The area does not change.
    let turned = json!({"kind":"rounded_rectangle","centre":[0,0],"width":40,"height":20,"radius":5,"rotation":30});
    let volume = extruded_volume(json!({"outer": turned}), &[]).unwrap();
    assert!((volume - expected).abs() < 1e-6, "{volume}");
}

#[test]
fn rounded_rectangle_works_as_a_hole() {
    // A 60 x 40 rectangle with the 40 x 20 rounded rectangle removed.
    let outer = json!({"kind":"rectangle","origin":[-30,-20],"size":[60,40]});
    let expected = (2400. - (800. - 25. * (4. - PI))) * 10.;
    let volume = extruded_volume(
        json!({"outer": outer, "holes": [rounded_rectangle(json!(5))]}),
        &[],
    )
    .unwrap();
    assert!((volume - expected).abs() < 1e-6, "{volume}");
}

#[test]
fn rounded_regular_hexagon_matches_the_corner_formula() {
    // Sharp area is (n / 2) R^2 sin(2 pi / n). Each corner removes r^2 (tan(pi / n) - pi / n).
    // R 10 and r 1 with n 6 give 259.8076 - 0.3225 mm2.
    let n = 6.;
    let sharp = n / 2. * 100. * (2. * PI / n).sin();
    let removed = n * (((PI / n).tan()) - PI / n);
    let profile = json!({"outer":{"kind":"regular_polygon","centre":[0,0],"radius":10,"sides":6,"rotation":0,"corner_radius":1}});
    let volume = extruded_volume(profile, &[]).unwrap();
    assert!((volume - (sharp - removed) * 10.).abs() < 1e-6, "{volume}");
}

#[test]
fn rounded_square_polygon_matches_the_corner_formula() {
    // A 10 x 10 square with r 4 removes 4 r^2 (1 - pi / 4) = r^2 (4 - pi).
    let profile = json!({"outer":{"kind":"polygon","points":[[0,0],[10,0],[10,10],[0,10]],"corner_radius":4}});
    let volume = extruded_volume(profile, &[]).unwrap();
    let expected = (100. - 16. * (4. - PI)) * 10.;
    assert!((volume - expected).abs() < 1e-6, "{volume}");
}

#[test]
fn zero_or_absent_corner_radius_keeps_sharp_corners() {
    let sharp = extruded_volume(
        json!({"outer":{"kind":"regular_polygon","centre":[0,0],"radius":10,"sides":6,"rotation":0}}),
        &[],
    )
    .unwrap();
    let zero = extruded_volume(
        json!({"outer":{"kind":"regular_polygon","centre":[0,0],"radius":10,"sides":6,"rotation":0,"corner_radius":0}}),
        &[],
    )
    .unwrap();
    assert!((sharp - zero).abs() < 1e-9);
    assert!((sharp - 2598.076211).abs() < 1e-3, "{sharp}");
}

#[test]
fn radius_too_large_for_the_edges_is_refused_with_the_loop_and_limit() {
    // Half the shorter side is 10 mm. A radius of 10.5 mm does not fit.
    let error = extruded_volume(json!({"outer": rounded_rectangle(json!(10.5))}), &[]).unwrap_err();
    assert!(error.contains("rounded_rectangle"), "{error}");
    assert!(error.contains("10.000 mm"), "{error}");
    // Each square edge is 10 mm. Two corners of r 5 use the whole edge.
    let error = extruded_volume(
        json!({"outer":{"kind":"polygon","points":[[0,0],[10,0],[10,10],[0,10]],"corner_radius":6}}),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("polygon"), "{error}");
    assert!(error.contains("5.000 mm"), "{error}");
    // The apothem of a regular hexagon with R 10 is 8.660 mm.
    let error = extruded_volume(
        json!({"outer":{"kind":"regular_polygon","centre":[0,0],"radius":10,"sides":6,"rotation":0,"corner_radius":9}}),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("regular_polygon"), "{error}");
    assert!(error.contains("8.660 mm"), "{error}");
}

#[test]
fn expression_radius_follows_a_parameter() {
    let profile = json!({"outer": rounded_rectangle(json!("radius"))});
    let five = extruded_volume(profile.clone(), &[("radius", "5")]).unwrap();
    let six = extruded_volume(profile.clone(), &[("radius", "6")]).unwrap();
    assert!(
        (five - (800. - 25. * (4. - PI)) * 10.).abs() < 1e-6,
        "{five}"
    );
    assert!((six - (800. - 36. * (4. - PI)) * 10.).abs() < 1e-6, "{six}");
    let error = extruded_volume(profile, &[("radius", "11")]).unwrap_err();
    assert!(error.contains("rounded_rectangle"), "{error}");
}

#[test]
fn expression_corner_radius_follows_a_parameter() {
    let profile = json!({"outer":{"kind":"regular_polygon","centre":[0,0],"radius":10,"sides":6,"rotation":0,"corner_radius":"r"}});
    let volume = extruded_volume(profile, &[("r", "1")]).unwrap();
    let expected = (259.8076211353316 - 6. * ((PI / 6.).tan() - PI / 6.)) * 10.;
    assert!((volume - expected).abs() < 1e-6, "{volume}");
}

#[test]
fn concave_corner_adds_material_inside_the_outline() {
    // An L-shape of 75 mm2 has five convex corners and one reflex corner.
    // Each convex corner removes r^2 (1 - pi / 4). The reflex corner adds the same amount.
    let l_shape = json!({"outer":{"kind":"polygon","points":[[0,0],[10,0],[10,5],[5,5],[5,10],[0,10]],"corner_radius":1}});
    let volume = extruded_volume(l_shape, &[]).unwrap();
    let expected = (75. - 4. * (1. - PI / 4.)) * 10.;
    assert!((volume - expected).abs() < 1e-6, "{volume}");
}
