#[path = "../examples/common/mod.rs"]
mod common;
use serde_json::json;

#[test]
fn concurrent_headless_renders_keep_their_camera_and_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Render isolation".into())).unwrap();
    let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(json!({
        "name":"Box","colour":[120,160,180],"geometry":{"op":"box","size":[10,20,30]}
    }))
    .unwrap();
    project.put_part("box".into(), part).unwrap();
    let bodies = &project.bodies;
    let mut expected = Vec::new();
    for (index, view) in [faber_scene::View::Top, faber_scene::View::Front]
        .into_iter()
        .enumerate()
    {
        let path = directory.path().join(format!("reference-{index}.png"));
        faber_wiring::render_view(bodies, &path, None, 128, 96, view).unwrap();
        expected.push((view, std::fs::read(path).unwrap()));
    }
    assert_ne!(expected[0].1, expected[1].1);
    std::thread::scope(|scope| {
        for index in 0..4 {
            let (view, bytes) = &expected[index % 2];
            let path = directory.path().join(format!("parallel-{index}.png"));
            scope.spawn(move || {
                faber_wiring::render_view(bodies, &path, None, 128, 96, *view).unwrap();
                assert_eq!(std::fs::read(path).unwrap(), *bytes);
            });
        }
    });
}

#[test]
fn render_can_frame_one_saved_or_candidate_part() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut client =
        common::Client::start(env!("CARGO_BIN_EXE_faber").into(), root.join("calls.jsonl"));
    client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"render-test","version":"1"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client.tool("open", json!({"path":root,"create":true,"name":"Targets"}));
    let part =
        json!({"name":"Box","colour":[120,160,180],"geometry":{"op":"box","size":[10,20,30]}});
    let mut far = part.clone();
    far["placement"] = json!({"translation":[1000,0,0]});
    client.tool("put_parts", json!({"parts":{"box":part,"far":far}}));
    let measured = client.tool("measure", json!({"a":"box","b":"far"}));
    assert_eq!(measured["coordinates"], "world");
    assert!((measured["distance_mm"].as_f64().unwrap() - 990.).abs() < 1e-7);
    assert_eq!(measured["a"], "box");
    assert_eq!(measured["b"], "far");
    let report = client.tool(
        "render",
        json!({"path":root.join("selected.png"),"part":"box","width":128,"height":96}),
    );
    assert_eq!(report["part"], "box");
    let selected = std::fs::read(root.join("selected.png")).unwrap();
    client.tool(
        "render",
        json!({"path":root.join("all.png"),"width":128,"height":96}),
    );
    assert_ne!(selected, std::fs::read(root.join("all.png")).unwrap());
    let before = std::fs::read(root.join("faber.json")).unwrap();
    client.tool(
        "preview_batch",
        json!({
            "operations":[{"tool":"put_part","id":"candidate","part":part}],
            "render":{"path":root.join("candidate.png"),"part":"candidate","width":128,"height":96}
        }),
    );
    assert_eq!(selected, std::fs::read(root.join("candidate.png")).unwrap());
    assert_eq!(before, std::fs::read(root.join("faber.json")).unwrap());
    let missing = client.request(
        "tools/call",
        json!({"name":"render","arguments":{
            "path":root.join("missing.png"),"part":"candidate","width":128,"height":96
        }}),
    );
    assert_eq!(missing["isError"], true);
    assert!(!root.join("missing.png").exists());
    client.tool("remove_part", json!({"id":"far"}));
    client.tool(
        "render",
        json!({"path":root.join("single.png"),"width":128,"height":96}),
    );
    assert_eq!(selected, std::fs::read(root.join("single.png")).unwrap());
}

#[test]
fn mass_properties_over_stdio_read_the_placed_solid_without_saving() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut client =
        common::Client::start(env!("CARGO_BIN_EXE_faber").into(), root.join("calls.jsonl"));
    client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"mass-test","version":"1"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client.tool("open", json!({"path":root,"create":true,"name":"Mass"}));
    let part = json!({"name":"Box","colour":[120,160,180],"geometry":{"op":"box","size":[10,20,30]},
        "placement":{"translation":[5,0,0]}});
    client.tool("put_parts", json!({"parts":{"box":part}}));
    let before = std::fs::read(root.join("faber.json")).unwrap();
    let mass = client.tool("mass_properties", json!({"id":"box","density_g_cm3":1}));
    assert_eq!(mass["coordinates"], "world");
    assert!((mass["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-7);
    assert!((mass["area_mm2"].as_f64().unwrap() - 2200.).abs() < 1e-7);
    assert!((mass["mass_g"].as_f64().unwrap() - 6.).abs() < 1e-9);
    assert!((mass["inertia_kg_mm2"][0][0].as_f64().unwrap() - 0.65).abs() < 1e-9);
    let plain = client.tool("mass_properties", json!({"id":"box"}));
    assert!(plain.get("mass_g").is_none());
    assert!(plain.get("inertia_kg_mm2").is_none());
    for arguments in [
        json!({"id":"missing"}),
        json!({"id":"box","density_g_cm3":0}),
    ] {
        let refused = client.request(
            "tools/call",
            json!({"name":"mass_properties","arguments":arguments}),
        );
        assert_eq!(refused["isError"], true);
    }
    assert_eq!(before, std::fs::read(root.join("faber.json")).unwrap());
}
