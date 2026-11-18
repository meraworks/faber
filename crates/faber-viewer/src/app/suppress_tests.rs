use super::*;
use egui_kittest::kittest::Queryable;
use serde_json::json;

#[test]
fn feature_list_suppresses_and_unsuppresses_a_feature() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Suppress GUI".into())).unwrap();
    let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"history","output":"out","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
        {"id":"out","geometry":{"op":"box","size":[10,20,30]}}
    ]}}))
    .unwrap();
    project.put_part("block".into(), part).unwrap();
    let data = project.data.clone();
    let bodies = project.bodies.clone();
    drop(project);
    let root = directory.path().to_owned();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440., 1100.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("Model viewport").click();
    for _ in 0..3 {
        harness.step();
    }
    assert_eq!(harness.state().selected.as_deref(), Some("block"));
    // The output row also has a Suppress button, but it is disabled. The first match is the base row.
    harness
        .query_all_by_label("Suppress")
        .next()
        .unwrap()
        .click();
    for _ in 0..200 {
        harness.step();
        if harness.query_by_label("Unsuppress").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.get_by_label("Unsuppress");
    let saved = faber_wiring::read(directory.path()).unwrap().0;
    let faber_model::Geometry::History { features, .. } = &saved.parts["block"].geometry else {
        panic!("Part has no history");
    };
    assert!(features[0].suppressed);
    assert!(!features[1].suppressed);
}
