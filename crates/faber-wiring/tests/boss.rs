mod plastic_support;
use faber_kernel_occt::OcctKernel;
use faber_ports::Kernel;
use plastic_support::build;
use serde_json::{Value, json};

fn boss() -> Value {
    json!({"op":"boss","body":"plate","plane":"top","at":[20,20],"diameter":8,"height":10,"hole_diameter":3,"hole_depth":8})
}

#[test]
fn plain_boss_volume() {
    let p = build(boss()).unwrap();
    let expected = (16. * 10. - 2.25 * 8.) * std::f64::consts::PI;
    assert!((p.bodies[0].volume - 3200. - expected).abs() < expected * 0.005);
    assert_eq!(
        OcctKernel
            .properties(p.bodies[0].shape.as_ref().unwrap())
            .unwrap()
            .solids,
        1
    );
}

#[test]
fn boss_with_four_ribs() {
    let mut op = boss();
    op["at"] = json!([4, 20]);
    let plain = build(op.clone()).unwrap().bodies[0].volume;
    op["ribs"] = json!({"count":4,"thickness":1.5,"length":6,"height":6});
    let p = build(op).unwrap();
    assert!(p.bodies[0].volume > plain + 4. * 5.9 * 1.5 * 6.);
    assert!((p.bodies[0].bounds[0][0] + 6.).abs() < 1e-5);
    let props = OcctKernel
        .properties(p.bodies[0].shape.as_ref().unwrap())
        .unwrap();
    assert!(props.valid && props.solids == 1);
    let (_, faces) = p.feature_topology("plate", "attachment").unwrap();
    assert!(
        faces
            .iter()
            .filter_map(|f| f.cylinder.as_ref())
            .any(|c| c.internal && (c.radius - 1.5).abs() < 1e-6 && (c.length - 8.).abs() < 1e-6)
    );
}

#[test]
fn drafted_boss_is_narrower_at_the_top() {
    let mut op = boss();
    op["draft_deg"] = json!(2);
    let p = build(op).unwrap();
    let (edges, _) = p.feature_topology("plate", "attachment").unwrap();
    let expected = 4. - 10. * 2_f64.to_radians().tan();
    assert!(edges.iter().any(|e| (e.centre[2] - 12.).abs() < 1e-6
        && e.radius.is_some_and(|r| (r - expected).abs() < 0.01)));
}

#[test]
fn boss_hole_must_be_smaller() {
    let mut op = boss();
    op["hole_diameter"] = json!(8);
    let error = build(op).err().unwrap();
    assert!(
        error.contains("Boss hole 8 mm must be less than the boss diameter 8 mm."),
        "{error}"
    );
}

#[test]
fn boss_fillet_adds_material_only_at_the_root() {
    let mut op = boss();
    let sharp = build(op.clone()).unwrap().bodies[0].volume;
    op["fillet"] = json!(1);
    let p = build(op).unwrap();
    assert!(p.bodies[0].volume > sharp + 3.0);
    let (edges, _) = p.feature_topology("plate", "attachment").unwrap();
    assert!(edges.iter().any(
        |e| (e.centre[2] - 3.).abs() < 1e-5 && e.radius.is_some_and(|r| (r - 4.).abs() < 1e-5)
    ));
}

#[test]
fn rib_count_rebuilds_with_a_parameter_and_refuses_a_fraction() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Boss ribs".into())).unwrap();
    p.set_params([("n".into(), "2".into())].into()).unwrap();
    let mut op = boss();
    op["ribs"] = json!({"count":"n","thickness":1.5,"length":6,"height":6});
    p.put_part("plate".into(), plastic_support::plate(op))
        .unwrap();
    let two = p.bodies[0].volume;
    p.set_params([("n".into(), "4".into())].into()).unwrap();
    assert!(p.bodies[0].volume > two + 100.);
    let before = p.data.revision;
    assert!(p.set_params([("n".into(), "2.5".into())].into()).is_err());
    assert_eq!(p.data.revision, before);
}
