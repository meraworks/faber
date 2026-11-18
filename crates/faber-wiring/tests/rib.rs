use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::{Value, json};
const SHELL: f64 = 40. * 40. * 20. - 36. * 36. * 18.;
fn tray(plane: Value, op: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Tray","colour":[120,140,155],"geometry":{"op":"history","output":"wall","features":[
        {"id":"base","geometry":{"op":"box","size":["width",40,20]}},
        {"id":"shell","geometry":{"op":"shell","source":"base","faces":{"normal":[0,0,1],"expect":1,"tolerance":0.00001},"thickness":2}},
        {"id":"work","geometry":{"op":"plane","placement":plane}},
        {"id":"wall","geometry":op}
    ]}})).unwrap()
}
fn web(lines: Value, thickness: f64) -> Part<Scalar> {
    tray(
        json!({"translation":[0,0,20]}),
        json!({"op":"web","body":"shell","plane":"work","lines":lines,"thickness":thickness}),
    )
}
fn rib(end: [f64; 2]) -> Part<Scalar> {
    tray(
        json!({"translation":[0,20,0],"rotation":[90,0,0]}),
        json!({"op":"rib","body":"shell","plane":"work","lines":[[[12,2],end]],"side":"symmetric","thickness":3}),
    )
}
fn project(dir: &std::path::Path) -> faber_core::Project {
    let mut p = faber_wiring::open(dir, true, Some("Walls".into())).unwrap();
    p.set_params([("width".into(), "40".into())].into())
        .unwrap();
    p
}
fn growth(p: &faber_core::Project, expected: f64) {
    let v = p.bodies[0].volume - SHELL;
    assert!((v - expected).abs() < expected * 0.01, "{v} != {expected}");
}
#[test]
fn web_across_the_box() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    p.put_part("tray".into(), web(json!([[[2, 20], [38, 20]]]), 2.))
        .unwrap();
    growth(&p, 1296.);
}
#[test]
fn two_crossing_webs() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    p.put_part(
        "tray".into(),
        web(json!([[[2, 20], [38, 20]], [[20, 2], [20, 38]]]), 2.),
    )
    .unwrap();
    growth(&p, 2. * 1296. - 72.);
}
#[test]
fn web_stops_at_an_overhanging_shelf() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    let mut recipe = serde_json::to_value(web(json!([[[2, 20], [38, 20]]]), 2.)).unwrap();
    let features = recipe["geometry"]["features"].as_array_mut().unwrap();
    features.splice(2..2,[
        json!({"id":"shelf","geometry":{"op":"box","size":[22,4,2]}}),
        json!({"id":"placed","geometry":{"op":"transform","source":"shelf","placement":{"translation":[0,18,10]}}}),
        json!({"id":"support","geometry":{"op":"union","left":"shell","right":"placed"}})
    ]);
    features.last_mut().unwrap()["geometry"]["body"] = json!("support");
    p.put_part(
        "tray".into(),
        serde_json::from_value::<Part<Scalar>>(recipe).unwrap(),
    )
    .unwrap();
    let shelf = 20. * 4. * 2.;
    let wall = 2. * (20. * 8. + 16. * 18.);
    assert!((p.bodies[0].volume - SHELL - shelf - wall).abs() < 1e-4);
}
#[test]
fn rib_in_a_corner() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    p.put_part("tray".into(), rib([2., 12.])).unwrap();
    growth(&p, 10. * 10. * 0.5 * 3.);
    let solid = faber_kernel_occt::OcctKernel
        .properties(p.bodies[0].shape.as_ref().unwrap())
        .unwrap();
    assert!(solid.valid);
    assert_eq!(solid.solids, 1);
}

#[test]
fn rib_side_sets_the_wall_position() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    for (side, far_y) in [("left", 17.), ("right", 23.)] {
        let recipe = tray(
            json!({"translation":[0,20,0],"rotation":[90,0,0]}),
            json!({"op":"rib","body":"shell","plane":"work","lines":[[[12,2],[2,12]]],"side":side,"thickness":3}),
        );
        p.put_part("tray".into(), recipe).unwrap();
        let (_, faces) = p.feature_topology("tray", "wall").unwrap();
        for y in [20., far_y] {
            assert!(
                faces
                    .iter()
                    .any(|f| (f.centre[1] - y).abs() < 1e-6 && (f.area - 50.).abs() < 1e-6)
            );
        }
    }
}
#[test]
fn draft_changes_both_web_flanks() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    let recipe = tray(
        json!({"translation":[0,0,20]}),
        json!({"op":"web","body":"shell","plane":"work","lines":[[[2,20],[38,20]]],"thickness":3,"draft_deg":2}),
    );
    p.put_part("tray".into(), recipe).unwrap();
    let (_, faces) = p.feature_topology("tray", "wall").unwrap();
    let flank_count = faces
        .iter()
        .filter_map(|f| f.normal)
        .filter(|n| {
            (n[1].abs() - 2_f64.to_radians().cos()).abs() < 1e-6
                && (n[2].abs() - 2_f64.to_radians().sin()).abs() < 1e-6
        })
        .count();
    assert_eq!(flank_count, 2);
}
#[test]
fn rib_that_misses_the_wall_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    let error = p.put_part("tray".into(), rib([7., 12.])).unwrap_err();
    assert!(
        error.contains("The rib does not reach the body at the end of line 1."),
        "{error}"
    );
}
#[test]
fn rib_thicker_than_the_cavity_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    let err = p
        .put_part("tray".into(), web(json!([[[2, 20], [38, 20]]]), 37.))
        .unwrap_err();
    assert!(err.contains("thickness"), "{err}");
}
#[test]
fn rib_follows_a_parameter() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    p.put_part("tray".into(), web(json!([[[2, 20], ["width-2", 20]]]), 2.))
        .unwrap();
    p.set_params([("width".into(), "55".into())].into())
        .unwrap();
    let v = p.bodies[0].volume;
    assert!((v - (55. * 40. * 20. - 51. * 36. * 18. + 51. * 2. * 18.)).abs() < 1e-4);
}
#[test]
fn suppressed_rib_gives_the_shell() {
    let d = tempfile::tempdir().unwrap();
    let mut p = project(d.path());
    p.put_part("tray".into(), rib([2., 12.])).unwrap();
    p.set_suppressed("tray", "wall", true, false).unwrap();
    assert!((p.bodies[0].volume - SHELL).abs() < 1e-5);
}
