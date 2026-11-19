use faber_core::Project;
use faber_kernel_occt::OcctKernel;
use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::{Value, json};

fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Sheet test".into())).unwrap();
    (dir, p)
}
fn flange(id: &str, on: &str, edge: u32, length: f64, angle: f64) -> Value {
    json!({"id":id,"on":on,"edge":edge,"length":length,"angle_deg":angle})
}
fn part(thickness: Value, flanges: Vec<Value>) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Bracket","colour":[100,130,140],"geometry":{
        "op":"sheet_metal","rule":{"thickness":thickness,"bend_radius":2},
        "base":{"outer":{"kind":"rectangle","origin":[0,0],"size":[50,40]}},"flanges":flanges}}))
    .unwrap()
}
fn dimensions(p: &Project) -> [f64; 3] {
    let [lo, hi] = p.bodies[0].bounds;
    std::array::from_fn(|i| hi[i] - lo[i])
}
fn one_solid(p: &Project) {
    let props = OcctKernel
        .properties(p.bodies[0].shape.as_ref().unwrap())
        .unwrap();
    assert!(props.valid);
    assert_eq!(props.solids, 1);
}
#[test]
fn l_bracket_volume() {
    let (_d, mut p) = project();
    p.put_part(
        "bracket".into(),
        part(json!(2), vec![flange("side", "base", 2, 30., 90.)]),
    )
    .unwrap();
    one_solid(&p);
    let expected = 2. * 40. * 80. + std::f64::consts::FRAC_PI_4 * (16. - 4.) * 40.;
    assert!((p.bodies[0].volume - expected).abs() < 1e-5);
    for (got, want) in dimensions(&p).into_iter().zip([54., 40., 34.]) {
        assert!((got - want).abs() < 1e-5, "{got} != {want}");
    }
}
#[test]
fn u_channel_is_symmetric() {
    let (_d, mut p) = project();
    p.put_part(
        "channel".into(),
        part(
            json!(2),
            vec![
                flange("right", "base", 2, 30., 90.),
                flange("left", "base", 4, 30., 90.),
            ],
        ),
    )
    .unwrap();
    one_solid(&p);
    let [lo, hi] = p.bodies[0].bounds;
    assert!((hi[0] + lo[0] - 50.).abs() < 1e-5);
    assert!((hi[0] - lo[0] - 58.).abs() < 1e-5);
}
#[test]
fn flange_on_a_flange() {
    let (_d, mut p) = project();
    p.put_part(
        "channel".into(),
        part(
            json!(2),
            vec![
                flange("side", "base", 2, 30., 90.),
                flange("top", "side", 3, 12., -90.),
            ],
        ),
    )
    .unwrap();
    one_solid(&p);
    let size = dimensions(&p);
    assert!(size[0] > 65. && size[2] < 40., "{size:?}");
}
#[test]
fn forty_five_degree_flange() {
    let (_d, mut p) = project();
    p.put_part(
        "angle".into(),
        part(json!(2), vec![flange("side", "base", 2, 30., 45.)]),
    )
    .unwrap();
    one_solid(&p);
    let expected = 6400. + std::f64::consts::FRAC_PI_8 * 12. * 40.;
    assert!((p.bodies[0].volume - expected).abs() < 1e-5);
}
#[test]
fn thickness_parameter_changes_the_part() {
    let (_d, mut p) = project();
    p.set_params([("gauge".into(), "2".into())].into()).unwrap();
    p.put_part(
        "sheet".into(),
        part(json!("gauge"), vec![flange("side", "base", 2, 30., 90.)]),
    )
    .unwrap();
    let before = p.bodies[0].volume;
    p.set_params([("gauge".into(), "3".into())].into()).unwrap();
    one_solid(&p);
    let size = dimensions(&p);
    assert!((size[0] - 55.).abs() < 1e-5 && (size[2] - 35.).abs() < 1e-5);
    assert!(p.bodies[0].volume > 1.49 * before);
}

#[test]
fn flange_edges_and_flat_size_use_the_new_pose_and_rule() {
    let (_d, mut p) = project();
    p.set_params([("gauge".into(), "2".into())].into()).unwrap();
    let mut bracket = part(json!("gauge"), vec![flange("side", "base", 2, 30., 90.)]);
    bracket.placement.translation = [100., 30., 7.].map(Scalar::Number);
    bracket.placement.rotation = [0., 0., 90.].map(Scalar::Number);
    p.put_part("sheet".into(), bracket).unwrap();
    let available = faber_wiring::sheet::inspect(&p.data, "sheet").unwrap().1;
    assert!(
        !available
            .iter()
            .any(|e| e.wall == "base" && e.edge == 2 || e.wall == "side" && e.edge == 1)
    );
    let base = available
        .iter()
        .find(|e| e.wall == "base" && e.edge == 1)
        .unwrap();
    for (seen, want) in base
        .ends
        .into_iter()
        .zip([[100., 30., 7.], [100., 80., 7.]])
    {
        assert!(
            seen.into_iter()
                .zip(want)
                .all(|(a, b)| (a - b).abs() < 1e-7)
        );
    }
    assert!(base.other.iter().all(|p| (p[2] - 9.).abs() < 1e-7));
    let before = faber_wiring::sheet::pattern(&p, "sheet").unwrap().size_mm[0];
    p.set_params([("gauge".into(), "3".into())].into()).unwrap();
    let after = faber_wiring::sheet::pattern(&p, "sheet").unwrap().size_mm[0];
    assert!((after - before - std::f64::consts::FRAC_PI_2 * 0.44).abs() < 1e-5);
}

#[test]
fn rejected_corner_preserves_the_folded_part_and_its_revision() {
    let (_d, mut p) = project();
    let a = flange("right", "base", 2, 30., 90.);
    p.put_part("bracket".into(), part(json!(2), vec![a.clone()]))
        .unwrap();
    let revision = p.data.revision;
    let volume = p.bodies[0].volume;
    let error = p
        .put_part(
            "bracket".into(),
            part(json!(2), vec![a, flange("back", "base", 3, 30., 90.)]),
        )
        .unwrap_err();
    assert!(error.contains("right") && error.contains("back"), "{error}");
    assert_eq!(p.data.revision, revision);
    assert_eq!(p.bodies[0].volume, volume);
}

#[test]
fn history_output_keeps_round_holes_exact_in_the_folded_solid() {
    let (_d, mut p) = project();
    let mut side = flange("side", "base", 2, 30., -90.);
    side["holes"] = json!([{"kind":"circle","centre":[20,15],"radius":3}]);
    let mut bracket = part(json!(2), vec![side]);
    bracket.geometry = faber_model::Geometry::History {
        features: vec![faber_model::Feature {
            id: "fold".into(),
            geometry: bracket.geometry,
            suppressed: false,
        }],
        output: "fold".into(),
    };
    p.put_part("bracket".into(), bracket).unwrap();
    let expected = 6400. + std::f64::consts::FRAC_PI_4 * 12. * 40. - std::f64::consts::PI * 18.;
    assert!((p.bodies[0].volume - expected).abs() < 1e-5);
    assert_eq!(
        faber_wiring::sheet::pattern(&p, "bracket")
            .unwrap()
            .holes
            .len(),
        1
    );
    one_solid(&p);
}

#[test]
fn inspection_of_one_sheet_needs_no_other_solid_or_mesh() {
    let (_d, p) = project();
    let mut data = p.data.clone();
    data.params.insert("gauge".into(), "2".into());
    data.parts.insert(
        "panel".into(),
        part(json!("gauge"), vec![flange("lip", "base", 2, 30., 90.)]),
    );
    let unrelated=serde_json::from_value(json!({"name":"External mesh","colour":[0,0,0],"geometry":{"op":"imported","asset":"not-loaded"}})).unwrap();
    data.parts.insert("unrelated".into(), unrelated);
    let (flat, edges) = faber_wiring::sheet::inspect(&data, "panel").unwrap();
    assert!((flat.size_mm[0] - 84.52389342).abs() < 0.0001);
    assert_eq!(edges.len(), 6);
}

#[test]
fn round_reliefs_remove_material_from_the_solid_and_its_pattern() {
    let (_d, mut p) = project();
    let mut flange = flange("side", "base", 2, 30., 90.);
    flange["inset_start"] = json!(5);
    flange["inset_end"] = json!(5);
    let mut bracket = part(json!(2), vec![flange]);
    p.put_part("bracket".into(), bracket.clone()).unwrap();
    let volume = p.bodies[0].volume;
    let flat = faber_wiring::sheet::pattern(&p, "bracket").unwrap();
    let faber_model::Geometry::SheetMetal { rule, .. } = &mut bracket.geometry else {
        unreachable!()
    };
    rule.relief = faber_model::Relief::Round {
        diameter: Scalar::Number(4.),
    };
    p.put_part("bracket".into(), bracket).unwrap();
    one_solid(&p);
    let new = faber_wiring::sheet::pattern(&p, "bracket").unwrap();
    // The two half-round cuts remove one full circle from the base wall.
    assert!((volume - p.bodies[0].volume - 8. * std::f64::consts::PI).abs() < 1e-5);
    assert!((flat.area_mm2 - new.area_mm2 - 4. * std::f64::consts::PI).abs() < 0.06);
}
