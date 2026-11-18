use faber_model::{Part, Scalar, study::Study};
use faber_wiring::study::{quick_stress, run_stress};
use serde_json::{Value, json};

fn selector(normal: [i32; 3]) -> Value {
    json!({"normal":normal,"tolerance":0.000001,"expect":1})
}

fn bracket(material: Option<&str>) -> (tempfile::TempDir, faber_core::Project, Study<Scalar>) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Bracket".into()).unwrap();
    p.set_params([("length".into(), "24".into())].into())
        .unwrap();
    let part:Part<Scalar>=serde_json::from_value(json!({
       "name":"Bracket","colour":[120,150,170],"material":material,
       "geometry":{"op":"history","features":[
           {"id":"wall","geometry":{"op":"box","size":[4,6,16]}},
           {"id":"shelf","geometry":{"op":"box","size":["length",6,4]}},
           {"id":"raised","geometry":{"op":"transform","source":"shelf","placement":{"translation":[0,0,12],"rotation":[0,0,0]}}},
           {"id":"join","geometry":{"op":"union","left":"wall","right":"raised"}}
       ],"output":"join"}
    })).unwrap();
    p.put_part("bracket".into(), part).unwrap();
    let mut back = selector([-1, 0, 0]);
    back["expect"] = json!(2);
    let s = serde_json::from_value(json!({
       "part":"bracket","fixtures":[{"kind":"fixed","face":back}],
       "loads":[{"kind":"force","face":selector([1,0,0]),"newtons":[0,0,-50]}],
       "resolution_mm":1, "print_direction":[0,0,1]
    }))
    .unwrap();
    (dir, p, s)
}

#[test]
fn bracket_report() {
    let (_d, p, mut s) = bracket(Some("pla"));
    let faber_model::study::StudyLoad::Force { face, .. } = &mut s.loads[0] else {
        unreachable!()
    };
    face.in_box = Some([[21., 0., 10.], [25., 6., 16.]].map(|row| row.map(Scalar::Number)));
    // Eight cells through the shelf resolve the stress at the inner corner.
    let (r, f) = quick_stress(&p, &s, Some(0.5)).unwrap();
    assert!(r.min_safety_factor.is_finite() && r.min_safety_factor > 0.);
    assert!(r.layer_safety_factor.is_some());
    assert!((r.reaction_n[2] - 50.).abs() < 0.5);
    assert!(r.hot_spots.len() <= 5);
    let hot = r.hot_spots[0].point;
    assert!(
        (hot[0] - 4.).hypot(hot[2] - 12.) <= 3.,
        "Corner stress at {hot:?}"
    );
    assert_eq!(f.vertex_von_mises.len(), p.bodies[0].mesh.vertices.len());
    assert_eq!(f.vertex_displacement.len(), f.vertex_von_mises.len());
}

#[test]
fn study_survives_a_parameter_change() {
    let (_dir, mut p, mut study) = bracket(Some("pla"));
    let faber_model::study::StudyLoad::Force { face, .. } = &mut study.loads[0] else {
        unreachable!()
    };
    face.near_centre = Some([
        Scalar::Expression("length".into()),
        Scalar::Number(3.),
        Scalar::Number(14.),
    ]);
    p.set_study("bend".into(), study).unwrap();
    let initial = run_stress(&p, "bend", None).unwrap().0;
    let cached = run_stress(&p, "bend", None).unwrap().0;
    assert_eq!(initial.max_displacement_mm, cached.max_displacement_mm);
    p.set_params([("length".into(), "32".into())].into())
        .unwrap();
    let longer = run_stress(&p, "bend", None).unwrap().0;
    assert!(longer.max_displacement_mm > initial.max_displacement_mm * 1.5);
}
#[test]
fn load_on_a_fixed_face_is_refused() {
    let (_d, p, mut s) = bracket(Some("pla"));
    let faber_model::study::StudyLoad::Force { face, .. } = &mut s.loads[0] else {
        unreachable!()
    };
    face.clone_from(&s.fixtures[0].face);
    assert!(
        quick_stress(&p, &s, None)
            .unwrap_err()
            .contains("fixed face")
    );
}
#[test]
fn missing_material_is_named() {
    let (_d, p, s) = bracket(None);
    assert_eq!(
        quick_stress(&p, &s, None).unwrap_err(),
        "Part bracket has no material. Set a material or give one in the study."
    );
}

#[test]
fn purchased_part_can_be_studied() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fastener-check");
    let dir = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(
        source.join("assets/f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e"),
    )
    .unwrap();
    let path = dir.path().join("screw.step");
    std::fs::write(&path, &bytes).unwrap();
    let mut p = faber_wiring::create(dir.path(), "Purchased screw".into()).unwrap();
    p.import_with_record(
        "screw".into(),
        "Screw".into(),
        &path,
        false,
        Some(faber_model::Purchased {
            designation: "M3 x 10".into(),
            licence: "CC BY 3.0".into(),
            source: "https://github.com/FreeCAD/FreeCAD-library".into(),
            mass_g: None,
        }),
    )
    .unwrap();
    let before = p.data.clone();
    let (_, faces) = p.unplaced_topology("screw").unwrap();
    let face = faces
        .iter()
        .filter(|f| f.normal.is_some())
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .unwrap();
    let fixture = faber_core::pick_selector::face_selector(&faces, &[face.index]).unwrap();
    let s = serde_json::from_value(json!({
        "part":"screw","material":"steel-s235","fixtures":[{"kind":"fixed","face":fixture}],
        "loads":[{"kind":"gravity","direction":[0,0,-1]}],"resolution_mm":0.6
    }))
    .unwrap();
    let report = quick_stress(&p, &s, None).unwrap().0;
    assert!(report.mass_g > 0. && report.max_displacement_mm > 0.);
    assert_eq!(p.data, before);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// Run in release mode to compare the shelf result at two cell sizes.
#[test]
#[ignore = "Measures the two full shelf grids"]
fn shelf_bracket_resolution_study() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/shelf-bracket-study");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("parts")).unwrap();
    for name in ["faber.json", "analysis.json", "parts/bracket.json"] {
        std::fs::copy(source.join(name), dir.path().join(name)).unwrap();
    }
    let project = faber_wiring::open(dir.path(), false, None).unwrap();
    let h = faber_sim::default_h(project.bodies[0].bounds);
    let mut coarse = None;
    for resolution in [h, h / 2.] {
        let start = std::time::Instant::now();
        let (r, _) = run_stress(&project, "shelf_load", Some(resolution)).unwrap();
        println!(
            "resolution_mm={} active_cells={} iterations={} seconds={:.3} peak_mpa={} safety_factor={} residual={}",
            r.resolution_mm,
            r.cells,
            r.iterations,
            start.elapsed().as_secs_f64(),
            r.max_von_mises_mpa,
            r.min_safety_factor,
            r.residual
        );
        assert!(r.max_von_mises_mpa.is_finite() && r.max_von_mises_mpa > 0.);
        if let Some(peak) = coarse {
            let difference: f64 = (peak - r.max_von_mises_mpa) / r.max_von_mises_mpa;
            println!(
                "Peak stress difference relative to the fine result: {:.2} %",
                100. * difference.abs()
            );
            assert!(
                difference.abs() < 0.20,
                "The peak stress changed by {:.2}% when the cell size was halved.",
                100. * difference.abs()
            );
        }
        coarse = Some(r.max_von_mises_mpa);
    }
}

#[test]
fn placed_part_keeps_its_face_selectors_and_uses_world_loads() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Placed bar".into()).unwrap();
    let mut part: Part<f64> = serde_json::from_value(json!({
       "name":"Bar","colour":[100,120,140],"material":"pla",
       "geometry":{"op":"box","size":[10,10,20]}
    }))
    .unwrap();
    p.put_part("bar".into(), part.clone()).unwrap();
    let mut s: Study<Scalar> = serde_json::from_value(json!({
       "part":"bar","fixtures":[{"kind":"fixed","face":selector([0,0,-1])}],
       "loads":[{"kind":"force","face":selector([0,0,1]),"newtons":[0,0,1000]}],"resolution_mm":2
    }))
    .unwrap();
    let reference = quick_stress(&p, &s, None).unwrap().0;
    part.placement.translation = [40., 50., 60.];
    part.placement.rotation = [0., 90., 0.];
    p.put_part("bar".into(), part).unwrap();
    let faber_model::study::StudyLoad::Force { newtons, .. } = &mut s.loads[0] else {
        unreachable!()
    };
    *newtons = [1000., 0., 0.].map(Scalar::Number);
    let rotated = quick_stress(&p, &s, None).unwrap().0;
    assert!((rotated.max_displacement_mm / reference.max_displacement_mm - 1.).abs() < 0.01);
    assert!((rotated.max_von_mises_mpa / reference.max_von_mises_mpa - 1.).abs() < 0.01);
    assert!((rotated.reaction_n[0] + 1000.).abs() < 0.01);
}
