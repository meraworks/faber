use faber_kernel_occt::OcctKernel;
use faber_model::Part;
use faber_ports::Kernel;
use serde_json::{Value, json};

fn geometry(cylinder: bool, outer: Value, mode: &str, depth: Value, at: [f64; 2]) -> Value {
    let (body, face) = if cylinder {
        (
            json!({"op":"cylinder","radius":20,"height":30}),
            json!({"cylinder_radius":20,"tolerance":0.000001,"expect":1}),
        )
    } else {
        (
            json!({"op":"box","size":[20,20,20]}),
            json!({"normal":[0,0,1],"tolerance":0.000001,"expect":1}),
        )
    };
    json!({"name":"Label","colour":[70,150,158],"geometry":{"op":"history","output":"mark","features":[
        {"id":"base","geometry":body},
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":outer}}},
        {"id":"mark","geometry":{"op":"emboss","body":"base","face":face,"sketch":"profile","depth":depth,"mode":mode,"at":at}}
    ]}})
}
fn rect(x: f64, width: f64) -> Value {
    json!({"kind":"rectangle","origin":[x,3],"size":[width,5]})
}
fn text(height: Value) -> Value {
    json!({"kind":"text","text":"AB","height":height,"at":[1,2],"align":"left","angle_deg":0})
}
fn part(value: Value) -> Part<faber_model::Scalar> {
    serde_json::from_value(value).unwrap()
}
fn fixture() -> (tempfile::TempDir, faber_core::Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Emboss".into())).unwrap();
    (dir, p)
}
#[test]
fn raised_rectangle_on_a_flat_face() {
    let (_dir, mut p) = fixture();
    p.put_part(
        "test".into(),
        part(geometry(false, rect(3., 10.), "raise", json!(1), [0., 0.])),
    )
    .unwrap();
    assert!((p.bodies[0].volume - 8050.).abs() < 1e-6);
    assert!((p.bodies[0].bounds[1][2] - 21.).abs() < 1e-5);
}
#[test]
fn raised_rectangle_on_a_cylinder() {
    let (_dir, mut p) = fixture();
    let v = std::f64::consts::PI * 20. * 20. * 30.;
    for arc_offset in [10., -8.] {
        p.put_part(
            "test".into(),
            part(geometry(
                true,
                rect(3., 10.),
                "raise",
                json!(1),
                [arc_offset, 2.],
            )),
        )
        .unwrap();
        let added = p.bodies[0].volume - v;
        assert!((added - 51.25).abs() < 0.02 * 51.25, "{added}");
        assert_eq!(
            OcctKernel
                .properties(p.bodies[0].shape.as_ref().unwrap())
                .unwrap()
                .solids,
            1
        );
    }
}
#[test]
fn engraved_text_and_a_text_parameter_rebuild_the_flat_and_wrapped_label() {
    for cylinder in [false, true] {
        let (_dir, mut p) = fixture();
        p.set_params([("height".into(), "6".into())].into())
            .unwrap();
        let outer = text(json!("height"));
        p.put_part("reference".into(),part(json!({"name":"Reference","colour":[20,50,80],"geometry":{"op":"history","output":"solid","features":[
            {"id":"outline","geometry":{"op":"sketch","profile":{"outer":outer}}},
            {"id":"solid","geometry":{"op":"extrude_sketch","source":"outline","vector":[0,0,0.6]}}
        ]}}))).unwrap();
        p.put_part(
            "test".into(),
            part(geometry(cylinder, outer, "engrave", json!(0.6), [8., 2.])),
        )
        .unwrap();
        let mut last = 0.;
        for h in ["6", "8"] {
            p.set_params([("height".into(), h.into())].into()).unwrap();
            let base = if cylinder {
                std::f64::consts::PI * 20. * 20. * 30.
            } else {
                8000.
            };
            let test = p.bodies.iter().find(|b| b.id == "test").unwrap();
            let reference = p.bodies.iter().find(|b| b.id == "reference").unwrap();
            let volume = base - test.volume;
            let expected = reference.volume * if cylinder { 1. - 0.6 / 40. } else { 1. };
            assert!(
                (expected - volume).abs() < 0.01 * expected,
                "{expected} {volume}"
            );
            assert!(volume > last);
            last = volume;
            assert_eq!(
                OcctKernel
                    .properties(test.shape.as_ref().unwrap())
                    .unwrap()
                    .solids,
                1
            );
            assert!((test.bounds[1][2] - if cylinder { 30. } else { 20. }).abs() < 1e-5);
        }
    }
}
#[test]
fn invalid_emboss_leaves_the_saved_part_unchanged() {
    let (dir, mut p) = fixture();
    p.put_part(
        "test".into(),
        part(geometry(false, rect(3., 10.), "raise", json!(1), [0., 0.])),
    )
    .unwrap();
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    for (cylinder, x, width, offset, expected) in [
        (false, 15., 10., 0., "outside"),
        (true, 0., 130., 0., "circumference"),
        (true, 1., 6., 29., "outside"),
    ] {
        let e = p
            .put_part(
                "test".into(),
                part(geometry(
                    cylinder,
                    rect(x, width),
                    "raise",
                    json!(1),
                    [0., offset],
                )),
            )
            .unwrap_err();
        assert!(e.to_lowercase().contains(expected), "{e}");
        assert_eq!(
            faber_wiring::project_fingerprint(dir.path()).unwrap(),
            before
        );
    }
}

#[test]
fn a_planar_profile_cannot_bridge_a_hole_in_the_target_face() {
    let (dir, mut p) = fixture();
    let mut model = geometry(false, rect(3., 10.), "raise", json!(1), [0., 0.]);
    let ops = model["geometry"]["features"].as_array_mut().unwrap();
    ops.splice(1..1, [
        json!({"id":"bore","geometry":{"op":"cylinder","radius":2,"height":20}}),
        json!({"id":"moved","geometry":{"op":"transform","source":"bore","placement":{"translation":[7,5,0]}}}),
        json!({"id":"plate","geometry":{"op":"difference","left":"base","right":"moved"}}),
    ]);
    ops.last_mut().unwrap()["geometry"]["body"] = json!("plate");
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let error = p.put_part("test".into(), part(model)).unwrap_err();
    assert!(error.to_lowercase().contains("outside"), "{error}");
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(dir.path()).unwrap()
    );
}

#[test]
fn a_wrapped_label_follows_edited_letters_and_undo() {
    let (_dir, mut p) = fixture();
    let mut letters = text(json!(5));
    letters["text"] = json!("I");
    p.put_part(
        "test".into(),
        part(geometry(
            true,
            letters.clone(),
            "engrave",
            json!(0.5),
            [4., 3.],
        )),
    )
    .unwrap();
    let one = p.bodies[0].volume;
    letters["text"] = json!("II");
    p.edit_feature(
        "test",
        "profile",
        serde_json::from_value(json!({
            "op":"sketch","profile":{"outer":letters}
        }))
        .unwrap(),
    )
    .unwrap();
    let stock = std::f64::consts::PI * 400. * 30.;
    let removed = stock - one;
    assert!(removed > 0.);
    assert!((stock - p.bodies[0].volume - 2. * removed).abs() < removed * 0.01);
    p.undo().unwrap();
    assert!((one - p.bodies[0].volume).abs() < 1e-6);
}
