mod plastic_support;
use plastic_support::build;
use serde_json::{Value, json};

fn hook() -> Value {
    json!({"op":"snap_hook","body":"plate","plane":"top","at":[20,20],"angle_deg":0,"length":15,"width":5,"thickness":2,"undercut":1.5,"hook_length":3})
}

#[test]
fn hook_volume() {
    let p = build(hook()).unwrap();
    let ramp = 1.5 / 30_f64.to_radians().tan();
    let expected = 5. * (15. * 2. + 1.5 * (3. - ramp * 0.5));
    assert!((p.bodies[0].volume - 3200. - expected).abs() < expected * 0.005);
}

#[test]
fn hook_points_along_the_angle() {
    let mut op = hook();
    op["at"] = json!([40, 40]);
    op["width"] = json!(2);
    let aligned = build(op.clone()).unwrap().bodies[0].bounds;
    op["angle_deg"] = json!(90);
    let turned = build(op).unwrap().bodies[0].bounds;
    for (actual, expected) in aligned[1]
        .into_iter()
        .chain(turned[1])
        .zip([42.5, 41., 17., 41., 42.5, 17.])
    {
        assert!((actual - expected).abs() < 1e-5, "{actual}, {expected}");
    }
}

#[test]
fn hook_uses_a_taper_and_a_real_root_fillet() {
    let mut op = hook();
    op["tip_thickness"] = json!(1);
    let tapered = build(op.clone()).unwrap();
    let sharp = tapered.bodies[0].volume;
    let taper_slope = (1. - 2.) / (2. * 15.);
    let ramp = 1.5 / (30_f64.to_radians().tan() + taper_slope);
    let (_, faces) = tapered.feature_topology("plate", "attachment").unwrap();
    assert!(
        faces
            .iter()
            .filter_map(|f| f.normal)
            .any(|n| (n[0] - 30_f64.to_radians().cos()).abs() < 1e-6 && (n[2] - 0.5).abs() < 1e-6)
    );
    assert!((sharp - 3200. - 5. * (15. * 1.5 + 1.5 * (3. - ramp * 0.5))).abs() < 1e-5);
    op["root_fillet"] = json!(0.5);
    let p = build(op).unwrap();
    assert!(p.bodies[0].volume > sharp + 0.1);
    op = hook();
    op["return_deg"] = json!(20);
    assert!(build(op).is_err());
}

#[test]
fn hook_strain() {
    let c = faber_wiring::hook_check(15., 2., 1.5, Some(0.015));
    assert!((c.strain - 0.02).abs() < 1e-9);
    assert_eq!(c.ok, Some(false));
}

#[test]
fn hook_strain_without_material() {
    let c = faber_wiring::hook_check(15., 2., 1.5, None);
    assert_eq!(c.ok, None);
    assert!(
        c.limits
            .iter()
            .any(|l| l == "No strain limit is known for this material.")
    );
    assert!(serde_json::to_value(c).unwrap().get("ok").is_none());
}
