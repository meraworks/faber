#[path = "../examples/common/mod.rs"]
mod common;
#[path = "../examples/common/curves.rs"]
mod curves;
use serde_json::json;

#[test]
fn curved_examples_build_through_stdio_and_keep_exact_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = common::Client::start(
        env!("CARGO_BIN_EXE_faber").into(),
        directory.path().join("transcript.jsonl"),
    );
    client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"curve-test","version":"1"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client.tool(
        "open",
        json!({"path":directory.path(),"create":true,"name":"Curves"}),
    );
    let report = client.tool("batch", json!({"operations":curves::operations()}));
    assert_eq!(report["part_count"], 3);
    let parts = report["parts"].as_array().unwrap();
    let volume = |id: &str| {
        parts.iter().find(|part| part["id"] == id).unwrap()["volume_mm3"]
            .as_f64()
            .unwrap()
    };
    assert!((volume("ring") - 6920. * std::f64::consts::PI).abs() < 1e-5);
    assert!(volume("nose") > 1000. && volume("nose") < 15000.);
    assert!(volume("wing") > 10000.);
    let wing_volume = volume("wing");
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    for body in &bodies {
        for reverse in [false, true] {
            let cut = faber_scene::section(
                &body.mesh,
                faber_scene::Section {
                    normal: [0., 1., 0.],
                    offset: 0.,
                    reverse,
                },
            );
            let cut = cut.unwrap_or_else(|error| panic!("{}: {error}", body.id));
            assert!(!cut.cap.triangles.is_empty(), "{}", body.id);
        }
    }
    client.tool("set_param", json!({"name":"span","expression":"300"}));
    let changed = client.tool("status", json!({}));
    let wing = changed["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "wing")
        .unwrap();
    assert!((wing["volume_mm3"].as_f64().unwrap() - wing_volume * 2.).abs() < 0.01);
    client.tool(
        "export",
        json!({"directory":directory.path().join("out"),"format":"step"}),
    );
    for id in ["nose", "ring", "wing"] {
        assert!(
            std::fs::read_to_string(directory.path().join(format!("out/{id}.step")))
                .unwrap()
                .starts_with("ISO-10303-21;")
        );
    }
}
