use faber_model::{Part, Scalar};
use serde_json::{Value, json};

const BOTTOM_RADIUS: f64 = 132.18;
const TOP_RADIUS: f64 = 150.;
const HEIGHT: f64 = 340.;

fn hexagon(radius: f64) -> Value {
    json!({"kind":"regular_polygon","centre":[0,0],"radius":radius,"sides":6,"rotation":0})
}

// Two hexagon sketches and a ruled loft between them.
fn taper_features() -> Vec<Value> {
    vec![
        json!({"id":"outer_bottom","geometry":{"op":"sketch","profile":{"outer":hexagon(BOTTOM_RADIUS)}}}),
        json!({"id":"outer_top","geometry":{"op":"sketch","profile":{"outer":hexagon(TOP_RADIUS)},"placement":{"translation":[0,0,HEIGHT]}}}),
        json!({"id":"outer","geometry":{"op":"loft_sketches","sources":["outer_bottom","outer_top"],"ruled":true}}),
    ]
}

fn taper_part(name: &str, features: Vec<Value>, output: &str) -> Part<Scalar> {
    serde_json::from_value(json!({"name":name,"colour":[100,140,170],"geometry":{"op":"history","output":output,"features":features}})).unwrap()
}

#[test]
fn ruled_hexagon_loft_reports_planar_sides_and_straight_edges() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Ruled loft".into())).unwrap();
    project
        .put_part(
            "taper".into(),
            taper_part("Taper", taper_features(), "outer"),
        )
        .unwrap();
    let (edges, faces) = project.feature_topology("taper", "outer").unwrap();
    assert_eq!(faces.len(), 8);
    assert!(faces.iter().all(|face| face.normal.is_some()));
    // Side faces lean outwards. The angle from the horizontal plane is the taper angle.
    let slope = (TOP_RADIUS - BOTTOM_RADIUS) * 30_f64.to_radians().cos() / HEIGHT;
    let expected_z = -slope / (1. + slope * slope).sqrt();
    let sides: Vec<_> = faces
        .iter()
        .filter(|face| face.normal.unwrap()[2].abs() < 0.999)
        .collect();
    assert_eq!(sides.len(), 6);
    for face in sides {
        let n = face.normal.unwrap();
        assert!(
            (n.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-9,
            "{n:?}"
        );
        assert!((n[2] - expected_z).abs() < 1e-6, "{n:?} {expected_z}");
        assert!(n[0] * face.centre[0] + n[1] * face.centre[1] > 0., "{n:?}");
    }
    assert_eq!(edges.len(), 18);
    for edge in &edges {
        let d = edge.direction.expect("every edge has a direction");
        assert!(
            (d.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-9,
            "{d:?}"
        );
    }
    let slanted_z = HEIGHT / (HEIGHT * HEIGHT + (TOP_RADIUS - BOTTOM_RADIUS).powi(2)).sqrt();
    let slanted = edges
        .iter()
        .filter(|edge| (edge.direction.unwrap()[2].abs() - slanted_z).abs() < 1e-6)
        .count();
    assert_eq!(slanted, 6);
}

#[test]
fn ruled_hexagon_loft_selectors_match_side_normal_and_slanted_direction() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Ruled loft selectors".into())).unwrap();
    project
        .put_part(
            "taper".into(),
            taper_part("Taper", taper_features(), "outer"),
        )
        .unwrap();
    let (edges, faces) = project.feature_topology("taper", "outer").unwrap();
    let side_normal = faces
        .iter()
        .filter_map(|face| face.normal)
        .find(|n| n[2].abs() < 0.999)
        .unwrap();
    let slanted_direction = edges
        .iter()
        .filter_map(|edge| edge.direction)
        .find(|d| d[2].abs() < 0.999 && d[2].abs() > 0.5)
        .unwrap();

    // One side face, selected by its normal, is thickened.
    let mut features = taper_features();
    features.push(json!({"id":"cap","geometry":{"op":"thicken","source":"outer","thickness":1,"face":{"normal":side_normal,"expect":1,"tolerance":0.000001}}}));
    project
        .put_part("cap".into(), taper_part("Cap", features, "cap"))
        .unwrap();

    // One slanted corner edge, selected by its direction, is filleted.
    let mut features = taper_features();
    features.push(json!({"id":"round","geometry":{"op":"fillet","source":"outer","edges":{"parallel":slanted_direction,"expect":1,"tolerance":0.000001},"radius":1}}));
    project
        .put_part("round".into(), taper_part("Round", features, "round"))
        .unwrap();
}
