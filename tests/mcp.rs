use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_tools_build_save_reopen_and_refuse_bad_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut request = |method: &str, params: Value| -> Value {
        id += 1;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "MCP server stopped");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        if method == "initialize" {
            writeln!(
                input,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .unwrap();
            input.flush().unwrap();
        }
        reply["result"].clone()
    };
    request(
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
    );
    let list = request("tools/list", json!({}));
    let metric = request(
        "tools/call",
        json!({"name":"metric_fastener","arguments":{"size":"m3"}}),
    );
    assert_ne!(metric["isError"], true, "{metric}");
    let metric: Value =
        serde_json::from_str(metric["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(metric["name"], "M3");
    assert_eq!(metric["pitch_mm"], 0.5);
    assert_eq!(metric["clearance_mm"]["medium"], 3.4);
    assert_eq!(metric["tap_drill_mm"], 2.5);
    let unknown = request(
        "tools/call",
        json!({"name":"metric_fastener","arguments":{"size":"M999"}}),
    );
    assert_eq!(unknown["isError"], true, "{unknown}");
    assert!(
        list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "put_part")
    );
    let open = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path(),"create":true,"name":"Test"}}),
    );
    assert_ne!(open["isError"], true);
    let reopened = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path()}}),
    );
    assert_ne!(reopened["isError"], true, "{reopened}");
    let part = json!({"name":"Base","geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":30},"colour":[80,100,120]});
    let put = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"base","part":part}}),
    );
    assert_ne!(put["isError"], true, "{put}");
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let base_revision = faber_files::read(dir.path()).unwrap().revision;
    let mut proposed = part.clone();
    proposed["geometry"]["height"] = json!(60);
    let preview = request(
        "tools/call",
        json!({"name":"preview_batch","arguments":{
            "expected_revision":base_revision,"operations":[{"tool":"put_part","id":"base","part":proposed}]
        }}),
    );
    assert_ne!(preview["isError"], true, "{preview}");
    let preview: Value =
        serde_json::from_str(preview["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(preview["saved"], false);
    assert_eq!(preview["base_revision"], base_revision);
    assert!(
        (preview["candidate"]["parts"][0]["volume_mm3"]
            .as_f64()
            .unwrap()
            - 12000.)
            .abs()
            < 1e-6
    );
    let section_path = dir.path().join("candidate-section.png");
    let image_preview = request(
        "tools/call",
        json!({"name":"preview_batch","arguments":{
            "expected_revision":base_revision,
            "operations":[{"tool":"put_part","id":"base","part":proposed}],
            "render":{"path":section_path,"width":128,"height":96,"view":"front",
                "section":{"normal":[0,0,1],"offset":40,"reverse":false}}
        }}),
    );
    assert_ne!(image_preview["isError"], true, "{image_preview}");
    assert_eq!(image_preview["content"][1]["mimeType"], "image/png");
    let png = base64::Engine::decode(
        &base64::prelude::BASE64_STANDARD,
        image_preview["content"][1]["data"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 128);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 96);
    assert_eq!(std::fs::read(section_path).unwrap(), png);
    let mut views = Vec::new();
    for view in ["top", "front"] {
        let rendered = request(
            "tools/call",
            json!({"name":"render","arguments":{
                "path":dir.path().join(format!("{view}.png")),"width":128,"height":96,"view":view,"inline":true
            }}),
        );
        assert_ne!(rendered["isError"], true, "{rendered}");
        let report: Value =
            serde_json::from_str(rendered["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["view"], view);
        views.push(rendered["content"][1]["data"].clone());
    }
    assert_ne!(views[0], views[1]);
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    let stale = request(
        "tools/call",
        json!({"name":"batch","arguments":{
            "expected_revision":base_revision+1,"operations":[{"tool":"set_param","name":"unused","expression":"1"}]
        }}),
    );
    assert_eq!(stale["isError"], true);
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    let mut bad = part;
    bad["geometry"]["height"] = json!(-2);
    let refused = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"base","part":bad}}),
    );
    assert_eq!(refused["isError"], true);
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    let export = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":dir.path().join("out")}}),
    );
    assert_ne!(export["isError"], true, "{export}");
    let bytes = std::fs::read(dir.path().join("out/base.stl")).unwrap();
    assert_eq!(bytes.len(), 84 + 12 * 50);
    let exact = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":dir.path().join("out"),"format":"step"}}),
    );
    assert_ne!(exact["isError"], true, "{exact}");
    let text = std::fs::read_to_string(dir.path().join("out/base.step")).unwrap();
    assert!(text.starts_with("ISO-10303-21;"));
    let status = request("tools/call", json!({"name":"status","arguments":{}}));
    let report: Value =
        serde_json::from_str(status["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6);
    let parameters = request(
        "tools/call",
        json!({"name":"set_param","arguments":{"name":"height","expression":"40"}}),
    );
    assert_ne!(parameters["isError"], true, "{parameters}");
    let expression_part = json!({"name":"Base","geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":"height"},"colour":[80,100,120]});
    let updated = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"base","part":expression_part,"verbose":true}}),
    );
    assert_ne!(updated["isError"], true, "{updated}");
    let report: Value =
        serde_json::from_str(updated["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 8000.).abs() < 1e-6);
    let params = request("tools/call", json!({"name":"params","arguments":{}}));
    let report: Value =
        serde_json::from_str(params["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["values"]["height"], 40.);
    let history = json!({"name":"Base","colour":[80,100,120],"geometry":{"op":"history","output":"box","features":[{"id":"box","geometry":{"op":"box","size":[10,20,"height"]}}]}});
    let replaced = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"base","part":history}}),
    );
    assert_ne!(replaced["isError"], true, "{replaced}");
    for (name, arguments) in [
        (
            "add_feature",
            json!({"part":"base","feature":{"id":"spare","geometry":{"op":"box","size":[1,2,3]}}}),
        ),
        (
            "move_feature",
            json!({"part":"base","id":"spare","before":"box"}),
        ),
        (
            "edit_feature",
            json!({"part":"base","id":"box","geometry":{"op":"box","size":[20,20,"height"]}}),
        ),
        ("delete_feature", json!({"part":"base","id":"spare"})),
    ] {
        let changed = request("tools/call", json!({"name":name,"arguments":arguments}));
        assert_ne!(changed["isError"], true, "{changed}");
    }
    let status = request("tools/call", json!({"name":"status","arguments":{}}));
    let report: Value =
        serde_json::from_str(status["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 16000.).abs() < 1e-6);
    let info = request(
        "tools/call",
        json!({"name":"part_info","arguments":{"id":"base"}}),
    );
    assert_ne!(info["isError"], true, "{info}");
    let info: Value = serde_json::from_str(info["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        info["recipe"]["geometry"]["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        info["recipe"]["geometry"]["features"][0]["geometry"]["size"][2],
        "height"
    );
    let missing = request(
        "tools/call",
        json!({"name":"part_info","arguments":{"id":"missing"}}),
    );
    assert_eq!(missing["isError"], true);
    let info_changed = request(
        "tools/call",
        json!({"name":"set_part_info","arguments":{"id":"base","note":"checked","colour":[1,2,3]}}),
    );
    assert_ne!(info_changed["isError"], true, "{info_changed}");
    let empty_info = request(
        "tools/call",
        json!({"name":"set_part_info","arguments":{"id":"base"}}),
    );
    assert_eq!(empty_info["isError"], true);
    let note = request(
        "tools/call",
        json!({"name":"part_info","arguments":{"id":"base"}}),
    );
    let note: Value = serde_json::from_str(note["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(note["recipe"]["note"], "checked");
    let topology = request(
        "tools/call",
        json!({"name":"topology","arguments":{"id":"base"}}),
    );
    assert_ne!(topology["isError"], true, "{topology}");
    let topology: Value =
        serde_json::from_str(topology["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(topology["faces"].as_array().unwrap().len(), 6);
    assert_eq!(topology["edges"].as_array().unwrap().len(), 12);
    assert!(topology["revision"].as_u64().unwrap() > 0);
    let source = &info["recipe"]["geometry"]["features"][0]["id"];
    let feature_topology = request(
        "tools/call",
        json!({"name":"topology","arguments":{"id":"base","feature":source}}),
    );
    assert_ne!(feature_topology["isError"], true, "{feature_topology}");
    let feature_topology: Value =
        serde_json::from_str(feature_topology["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(feature_topology["feature"], *source);
    assert_eq!(feature_topology["coordinates"], "part");
    assert_eq!(feature_topology["edges"].as_array().unwrap().len(), 12);
    for name in ["undo", "redo"] {
        let changed = request("tools/call", json!({"name":name,"arguments":{}}));
        assert_ne!(changed["isError"], true, "{changed}");
    }
    for (name, arguments) in [
        ("checkpoint", json!({"name":"baseline"})),
        ("set_param", json!({"name":"height","expression":"50"})),
        ("restore", json!({"name":"baseline"})),
    ] {
        let changed = request("tools/call", json!({"name":name,"arguments":arguments}));
        assert_ne!(changed["isError"], true, "{changed}");
    }
    let restored = request("tools/call", json!({"name":"params","arguments":{}}));
    let restored: Value =
        serde_json::from_str(restored["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(restored["values"]["height"], 40.);
    let changed = request(
        "tools/call",
        json!({"name":"batch","arguments":{"verbose":true,"operations":[
            {"tool":"set_param","name":"height","expression":"60"},
            {"tool":"edit_feature","part":"base","id":"box","geometry":{"op":"box","size":[10,20,"height"]}}
        ]}}),
    );
    assert_ne!(changed["isError"], true, "{changed}");
    let report: Value =
        serde_json::from_str(changed["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 12000.).abs() < 1e-6);
    let undone = request(
        "tools/call",
        json!({"name":"undo","arguments":{"verbose":true}}),
    );
    assert_ne!(undone["isError"], true, "{undone}");
    let report: Value =
        serde_json::from_str(undone["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 16000.).abs() < 1e-6);
    let checked = request(
        "tools/call",
        json!({"name":"interference","arguments":{"proximity_mm":0.1}}),
    );
    assert_ne!(checked["isError"], true, "{checked}");
    let report: Value =
        serde_json::from_str(checked["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["report"]["status"], "PASS");
    let invalid = request(
        "tools/call",
        json!({"name":"interference","arguments":{"proximity_mm":-1}}),
    );
    assert_eq!(invalid["isError"], true);
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let refused = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"strict":true,"id":"inside","part":{"name":"Inside","colour":[100,140,170],"geometry":{"op":"box","size":[1,1,1]},"placement":{"translation":[2,2,2]}}}}),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(
        refused["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Strict change refused"),
        "{refused}"
    );
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
    let added = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"inside","part":{"name":"Inside","colour":[100,140,170],"geometry":{"op":"box","size":[1,1,1]},"placement":{"translation":[2,2,2]}}}}),
    );
    assert_ne!(added["isError"], true, "{added}");
    let output = dir.path().join("checked-output");
    let refused = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":output}}),
    );
    assert_eq!(refused["isError"], true);
    assert!(!output.exists());
    let forced = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":output,"force":true}}),
    );
    assert_ne!(forced["isError"], true, "{forced}");
    assert!(output.join("inside.stl").exists());
    let removed = request(
        "tools/call",
        json!({"name":"remove_part","arguments":{"id":"inside"}}),
    );
    assert_ne!(removed["isError"], true);
    let pattern = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"pattern","part":{"name":"Row","colour":[100,140,170],"geometry":{"op":"history","output":"row","features":[
            {"id":"seed","geometry":{"op":"box","size":[2,3,4]}},
            {"id":"row","geometry":{"op":"linear_pattern","source":"seed","count":3,"step":[10,0,0]}}
        ]}}}}),
    );
    assert_ne!(pattern["isError"], true, "{pattern}");
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    let row = bodies.iter().find(|body| body.id == "pattern").unwrap();
    assert!((row.volume - 72.).abs() < 1e-5);
    let circular = request(
        "tools/call",
        json!({"name":"edit_feature","arguments":{
            "part":"pattern","id":"row","geometry":{"op":"circular_pattern","source":"seed","count":3,"origin":[0,0,0],"axis":[0,0,1],"angle":90}
        }}),
    );
    assert_ne!(circular["isError"], true, "{circular}");
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    let row = bodies.iter().find(|body| body.id == "pattern").unwrap();
    assert!((row.volume - 72.).abs() < 1e-5);
    assert!((row.bounds[0][0] + 3.).abs() < 1e-5);
    let mirrored = request(
        "tools/call",
        json!({"name":"edit_feature","arguments":{
            "part":"pattern","id":"row","geometry":{"op":"mirror","source":"seed","origin":[10,0,0],"normal":[1,0,0],"keep_source":true}
        }}),
    );
    assert_ne!(mirrored["isError"], true, "{mirrored}");
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    let row = bodies.iter().find(|body| body.id == "pattern").unwrap();
    assert!((row.volume - 48.).abs() < 1e-5);
    assert!((row.bounds[1][0] - 20.).abs() < 1e-5);
    let removed = request(
        "tools/call",
        json!({"name":"remove_part","arguments":{"id":"pattern"}}),
    );
    assert_ne!(removed["isError"], true);
    let imported = request(
        "tools/call",
        json!({"name":"import_part","arguments":{
            "id":"imported","name":"Imported block","path":dir.path().join("out/base.step")
        }}),
    );
    assert_ne!(imported["isError"], true, "{imported}");
    let (_, bodies) = faber_wiring::read(dir.path()).unwrap();
    assert!((bodies.iter().find(|b| b.id == "imported").unwrap().volume - 6000.).abs() < 1e-5);
    let removed = request(
        "tools/call",
        json!({"name":"remove_part","arguments":{"id":"imported"}}),
    );
    assert_ne!(removed["isError"], true);
    let saved = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let bad_path = dir.path().join("bad.step");
    std::fs::write(&bad_path, b"not a model").unwrap();
    let refused = request(
        "tools/call",
        json!({"name":"import_part","arguments":{
            "id":"bad","name":"Bad model","path":bad_path
        }}),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        saved
    );
    let status = request("tools/call", json!({"name":"status","arguments":{}}));
    assert_ne!(status["isError"], true);
    drop(input);
    assert!(child.wait().unwrap().success());
    let (_data, bodies) = faber_wiring::read(dir.path()).unwrap();
    assert_eq!(bodies.len(), 1);
}

#[test]
fn stdio_topology_filters_by_kind_selector_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut request = |method: &str, params: Value| -> Value {
        id += 1;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "MCP server stopped");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        if method == "initialize" {
            writeln!(
                input,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .unwrap();
            input.flush().unwrap();
        }
        reply["result"].clone()
    };
    request(
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
    );
    let open = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path(),"create":true,"name":"Topology"}}),
    );
    assert_ne!(open["isError"], true, "{open}");
    let part = json!({"name":"Base","geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":30},"colour":[80,100,120]});
    let put = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"base","part":part}}),
    );
    assert_ne!(put["isError"], true, "{put}");
    let faces = request(
        "tools/call",
        json!({"name":"topology","arguments":{"id":"base","kind":"faces",
            "face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001}}}),
    );
    assert_ne!(faces["isError"], true, "{faces}");
    let faces: Value = serde_json::from_str(faces["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(faces["faces"].as_array().unwrap().len(), 1, "{faces}");
    assert_eq!(faces["edges"].as_array().unwrap().len(), 0, "{faces}");
    let limited = request(
        "tools/call",
        json!({"name":"topology","arguments":{"id":"base","kind":"edges","limit":2}}),
    );
    assert_ne!(limited["isError"], true, "{limited}");
    let limited: Value =
        serde_json::from_str(limited["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(limited["edges"].as_array().unwrap().len(), 2, "{limited}");
    assert_eq!(limited["faces"].as_array().unwrap().len(), 0, "{limited}");
    assert_eq!(limited["total"]["edges"], 12, "{limited}");
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn stdio_overhang_finds_the_bar_underside_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut request = |method: &str, params: Value| -> Value {
        id += 1;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "MCP server stopped");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        if method == "initialize" {
            writeln!(
                input,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .unwrap();
            input.flush().unwrap();
        }
        reply["result"].clone()
    };
    request(
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
    );
    let open = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path(),"create":true,"name":"Overhang"}}),
    );
    assert_ne!(open["isError"], true, "{open}");
    // Two pillars of 10 by 10 by 20 under a bar of 40 by 10 by 10.
    let bridge = json!({"name":"Bridge","colour":[120,140,160],"geometry":{"op":"history","features":[
        {"id":"post","geometry":{"op":"box","size":[10,10,20]}},
        {"id":"far_post","geometry":{"op":"transform","source":"post",
            "placement":{"translation":[30,0,0],"rotation":[0,0,0]}}},
        {"id":"joined","geometry":{"op":"union","left":"post","right":"far_post"}},
        {"id":"bar_block","geometry":{"op":"box","size":[40,10,10]}},
        {"id":"bar","geometry":{"op":"transform","source":"bar_block",
            "placement":{"translation":[0,0,20],"rotation":[0,0,0]}}},
        {"id":"all","geometry":{"op":"union","left":"joined","right":"bar"}}
    ],"output":"all"}});
    let put = request(
        "tools/call",
        json!({"name":"put_part","arguments":{"id":"bridge","part":bridge}}),
    );
    assert_ne!(put["isError"], true, "{put}");
    let report = request(
        "tools/call",
        json!({"name":"overhang_analysis","arguments":{"part":"bridge"}}),
    );
    assert_ne!(report["isError"], true, "{report}");
    let report: Value =
        serde_json::from_str(report["content"][0]["text"].as_str().unwrap()).unwrap();
    let near = |value: &Value, want: f64| (value.as_f64().unwrap() - want).abs() < 0.5;
    assert!(near(&report["support_area_mm2"], 200.), "{report}");
    assert!(near(&report["bed_area_mm2"], 200.), "{report}");
    assert!(
        report["flattest_overhang_deg"].as_f64().unwrap() < 0.01,
        "{report}"
    );
    assert_eq!(report["regions_total"], 1, "{report}");
    assert_eq!(report["regions"][0]["part"], "bridge", "{report}");
    // An unknown part and a zero direction are refused.
    let unknown = request(
        "tools/call",
        json!({"name":"overhang_analysis","arguments":{"part":"nope"}}),
    );
    assert_eq!(unknown["isError"], true, "{unknown}");
    let zero = request(
        "tools/call",
        json!({"name":"overhang_analysis","arguments":{"direction":[0,0,0]}}),
    );
    assert_eq!(zero["isError"], true, "{zero}");
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn stdio_declared_contact_lets_an_overlap_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut request = |method: &str, params: Value| -> Value {
        id += 1;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "MCP server stopped");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        if method == "initialize" {
            writeln!(
                input,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .unwrap();
            input.flush().unwrap();
        }
        reply["result"].clone()
    };
    request(
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
    );
    let open = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path(),"create":true,"name":"Contact"}}),
    );
    assert_ne!(open["isError"], true, "{open}");
    let block = |z: f64| json!({"name":"Box","colour":[80,100,120],"geometry":{"op":"box","size":[10,10,10]},"placement":{"translation":[0,0,z]}});
    for (id, z) in [("box", 0.0), ("lid", 9.0)] {
        let put = request(
            "tools/call",
            json!({"name":"put_part","arguments":{"id":id,"part":block(z)}}),
        );
        assert_ne!(put["isError"], true, "{put}");
    }
    let export_dir = dir.path().join("out");
    let refused = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":export_dir}}),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("box") && text.contains("lid"), "{text}");
    assert!(text.contains("depth 1.000 mm"), "{text}");
    assert!(text.contains("set_contact"), "{text}");
    let declared = request(
        "tools/call",
        json!({"name":"set_contact","arguments":{"a":"box","b":"lid","allowed":true,"note":"Lid seats on the box","max_depth_mm":1.5}}),
    );
    assert_ne!(declared["isError"], true, "{declared}");
    let check = request(
        "tools/call",
        json!({"name":"interference","arguments":{"proximity_mm":0.1}}),
    );
    let check: Value = serde_json::from_str(check["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(check["report"]["status"], "PASS");
    let exported = request(
        "tools/call",
        json!({"name":"export","arguments":{"directory":export_dir}}),
    );
    assert_ne!(exported["isError"], true, "{exported}");
    assert_eq!(std::fs::read_dir(&export_dir).unwrap().count(), 2);
    drop(input);
    assert!(child.wait().unwrap().success());
}

/// A stdio session for the move tests. Tool errors come back as text.
struct Session {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: BufReader<std::process::ChildStdout>,
    id: u64,
}

impl Session {
    fn start(dir: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut session = Self {
            child,
            input,
            output,
            id: 0,
        };
        session.rpc(
            "initialize",
            json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
        );
        writeln!(
            session.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        session.input.flush().unwrap();
        session.ok(
            "open",
            json!({"path":dir.display().to_string(),"create":true,"name":"Move"}),
        );
        session
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "MCP server stopped");
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    /// Calls a tool. Returns the error flag and the parsed reply, or the text of an error.
    fn call(&mut self, tool: &str, arguments: Value) -> (bool, Value) {
        let result = self.rpc("tools/call", json!({"name":tool,"arguments":arguments}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let value = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
        (result["isError"] == true, value)
    }

    fn ok(&mut self, tool: &str, arguments: Value) -> Value {
        let (failed, value) = self.call(tool, arguments);
        assert!(!failed, "{tool}: {value}");
        value
    }

    /// Returns the error text. The call must fail.
    fn refused(&mut self, tool: &str, arguments: Value) -> String {
        let (failed, value) = self.call(tool, arguments);
        assert!(failed, "{tool} should refuse: {value}");
        value.as_str().unwrap().to_string()
    }

    fn finish(mut self) {
        drop(self.input);
        assert!(self.child.wait().unwrap().success());
    }
}

/// A box from the origin, with the given size and placement translation.
fn box_part(name: &str, size: [f64; 3], translation: [f64; 3]) -> Value {
    json!({"name":name,"geometry":{"op":"extrude","profile":{"outer":[[0,0],[size[0],0],[size[0],size[1]],[0,size[1]]]},"height":size[2]},"placement":{"translation":translation},"colour":[80,100,120]})
}

/// Checks flat numbers, such as a min corner then a max corner, within 1e-3 mm.
fn assert_near(actual: &Value, expected: &[f64]) {
    let flat: Vec<f64> = actual
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|value| match value {
            Value::Array(inner) => inner.iter().map(|x| x.as_f64().unwrap()).collect(),
            other => vec![other.as_f64().unwrap()],
        })
        .collect();
    assert_eq!(flat.len(), expected.len(), "{actual}");
    for (a, e) in flat.iter().zip(expected) {
        assert!((a - e).abs() < 1e-3, "{actual} != {expected:?}");
    }
}

#[test]
fn stdio_move_part_translates_turns_and_refuses_bad_modes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"block","part":box_part("Block", [10., 20., 30.], [0., 0., 0.])}),
    );
    let moved = s.ok("move_part", json!({"id":"block","translate":[5.,0.,2.]}));
    assert_near(
        &moved["changed"][0]["bounds_mm"],
        &[5., 0., 2., 15., 20., 32.],
    );
    assert_eq!(
        moved["changed"][0]["placement"]["translation"],
        json!([5., 0., 2.])
    );

    // Turn the block 90 degrees about Z. The centre of the box is (5, 10, 15).
    s.ok(
        "put_part",
        json!({"id":"turned","part":box_part("Turned", [10., 20., 30.], [0., 0., 0.])}),
    );
    let turned = s.ok(
        "move_part",
        json!({"id":"turned","rotate":{"axis":"z","angle_deg":90}}),
    );
    assert_near(
        &turned["changed"][0]["bounds_mm"],
        &[-5., 5., 0., 15., 15., 30.],
    );
    assert_eq!(
        turned["changed"][0]["placement"]["rotation"],
        json!([0., 0., 90.])
    );
    assert_near(
        &turned["changed"][0]["placement"]["translation"],
        &[15., 5., 0.],
    );

    s.ok(
        "put_part",
        json!({"id":"slide","part":box_part("Slide", [10., 20., 30.], [0., 0., 0.])}),
    );
    let slid = s.ok(
        "move_part",
        json!({"id":"slide","point_to_point":{"from":[0.,0.,0.],"to":[100.,0.,0.]}}),
    );
    assert_near(
        &slid["changed"][0]["bounds_mm"],
        &[100., 0., 0., 110., 20., 30.],
    );

    let placed = s.ok("move_part", json!({"id":"slide","position":[0.,0.,0.]}));
    assert_near(
        &placed["changed"][0]["bounds_mm"],
        &[-5., -10., -15., 5., 10., 15.],
    );

    let none = s.refused("move_part", json!({"id":"slide"}));
    assert!(none.contains("Found 0"), "{none}");
    let two = s.refused(
        "move_part",
        json!({"id":"slide","translate":[1.,0.,0.],"position":[0.,0.,0.]}),
    );
    assert!(two.contains("Found 2"), "{two}");

    s.ok("set_param", json!({"name":"L","expression":"5"}));
    s.ok(
        "put_part",
        json!({"id":"expr","part":{"name":"Expr","geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":30},"placement":{"translation":["L",0,0]},"colour":[80,100,120]}}),
    );
    let expression = s.refused("move_part", json!({"id":"expr","translate":[1.,0.,0.]}));
    assert!(expression.contains("translation[0]"), "{expression}");

    let stale = s.refused(
        "move_part",
        json!({"id":"slide","translate":[1.,0.,0.],"expected_revision":0}),
    );
    assert!(stale.contains("revision changed"), "{stale}");
    s.finish();
}

#[test]
fn stdio_move_part_copy_keeps_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"src","part":box_part("Source", [10., 20., 30.], [0., 0., 0.])}),
    );
    let copy = s.ok(
        "move_part",
        json!({"id":"src","translate":[0.,0.,50.],"copy_as":"copy"}),
    );
    assert_eq!(copy["changed"][0]["id"], "copy");
    assert_near(
        &copy["changed"][0]["bounds_mm"],
        &[0., 0., 50., 10., 20., 80.],
    );
    let source = s.ok("part_info", json!({"id":"src"}));
    assert_near(&source["bounds_mm"], &[0., 0., 0., 10., 20., 30.]);
    let taken = s.refused(
        "move_part",
        json!({"id":"src","translate":[1.,0.,0.],"copy_as":"copy"}),
    );
    assert!(taken.contains("already exists"), "{taken}");
    s.finish();
}

#[test]
fn stdio_move_part_refuses_a_copy_of_a_purchased_part() {
    let dir = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/fastener-check/assets")
        .join("f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e");
    let step = dir.path().join("screw.step");
    std::fs::copy(source, &step).unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "import_part",
        json!({
            "id": "screw",
            "name": "Screw",
            "path": step.display().to_string(),
            "purchased": {"source": "FreeCAD-library", "licence": "CC BY 3.0", "designation": "M3 x 10"}
        }),
    );
    let moved = s.ok("move_part", json!({"id":"screw","translate":[12.,0.,0.]}));
    assert_near(
        &moved["changed"][0]["placement"]["translation"],
        &[12., 0., 0.],
    );
    let refused = s.refused(
        "move_part",
        json!({"id":"screw","translate":[1.,0.,0.],"copy_as":"copy"}),
    );
    assert_eq!(
        refused,
        "Part screw is a purchased part. A copy is not permitted."
    );
    s.finish();
}

#[test]
fn stdio_align_parts_stacks_with_gap_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"base","part":box_part("Base", [40., 40., 10.], [0., 0., 0.])}),
    );
    s.ok(
        "put_part",
        json!({"id":"top","part":box_part("Top", [10., 10., 5.], [0., 0., 20.])}),
    );
    // The min face of top goes on the max face of base, plus a 2 mm gap.
    let stacked = s.ok(
        "align_parts",
        json!({"id":"top","target":"base","axes":["z"],"mode":"min_to_max","gap_mm":2.}),
    );
    assert_near(
        &stacked["changed"][0]["bounds_mm"],
        &[0., 0., 12., 10., 10., 17.],
    );
    let base = s.ok("part_info", json!({"id":"base"}));
    assert_near(&base["bounds_mm"], &[0., 0., 0., 40., 40., 10.]);

    let gap = s.refused(
        "align_parts",
        json!({"id":"top","target":"base","axes":["x"],"mode":"centre","gap_mm":1.}),
    );
    assert!(gap.contains("gap_mm applies only"), "{gap}");
    let itself = s.refused(
        "align_parts",
        json!({"id":"top","target":"top","axes":["x"],"mode":"min"}),
    );
    assert!(itself.contains("itself"), "{itself}");

    s.ok("undo", json!({}));
    let restored = s.ok("part_info", json!({"id":"top"}));
    assert_near(&restored["bounds_mm"], &[0., 0., 20., 10., 10., 25.]);
    s.finish();
}

#[test]
fn stdio_suppress_feature_skips_refuses_dependants_and_undoes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"block","part":{"name":"Suppress","colour":[100,140,170],"geometry":{"op":"history","output":"out","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
            {"id":"round","geometry":{"op":"fillet","source":"base","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":1}},
            {"id":"out","geometry":{"op":"box","size":[10,20,30]}}
        ]}}}),
    );
    let stale = s.refused(
        "suppress_feature",
        json!({"part":"block","id":"round","expected_revision":0}),
    );
    assert!(stale.contains("revision"), "{stale}");
    let blocked = s.refused("suppress_feature", json!({"part":"block","id":"base"}));
    assert!(blocked.contains("round"), "{blocked}");
    let output = s.refused("suppress_feature", json!({"part":"block","id":"out"}));
    assert!(
        output.contains("output feature cannot be suppressed"),
        "{output}"
    );
    s.ok("suppress_feature", json!({"part":"block","id":"round"}));
    let info = s.ok("part_info", json!({"id":"block"}));
    assert_eq!(
        info["recipe"]["geometry"]["features"][1]["suppressed"],
        true
    );
    assert!(
        info["recipe"]["geometry"]["features"][0]
            .get("suppressed")
            .is_none()
    );
    s.ok(
        "suppress_feature",
        json!({"part":"block","id":"base","cascade":true}),
    );
    let info = s.ok("part_info", json!({"id":"block"}));
    assert_eq!(
        info["recipe"]["geometry"]["features"][0]["suppressed"],
        true
    );
    s.ok("undo", json!({}));
    let info = s.ok("part_info", json!({"id":"block"}));
    assert!(
        info["recipe"]["geometry"]["features"][0]
            .get("suppressed")
            .is_none()
    );
    assert_eq!(
        info["recipe"]["geometry"]["features"][1]["suppressed"],
        true
    );
    s.ok(
        "suppress_feature",
        json!({"part":"block","id":"round","suppressed":false}),
    );
    let info = s.ok("part_info", json!({"id":"block"}));
    assert!(
        info["recipe"]["geometry"]["features"][1]
            .get("suppressed")
            .is_none()
    );
    s.finish();
}

#[test]
fn changing_tools_reply_with_changed_parts_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    for (id, x) in [("a", 0.), ("b", 20.), ("c", 40.)] {
        s.ok(
            "put_part",
            json!({"id":id,"part":box_part(id, [10., 10., 10.], [x, 0., 0.])}),
        );
    }
    let reply = s.ok("set_part_info", json!({"id":"b","name":"Bee"}));
    assert_eq!(reply["changed"].as_array().unwrap().len(), 1, "{reply}");
    assert_eq!(reply["changed"][0]["id"], "b");
    assert_eq!(reply["part_count"], 3);
    assert!(reply.get("parts").is_none(), "{reply}");
    let full = s.ok(
        "set_part_info",
        json!({"id":"b","name":"Bee again","verbose":true}),
    );
    assert_eq!(full["parts"].as_array().unwrap().len(), 3, "{full}");
    s.finish();
}

#[test]
fn every_changing_tool_checks_expected_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    for (id, x) in [("a", 0.), ("b", 20.), ("c", 40.)] {
        s.ok(
            "put_part",
            json!({"id":id,"part":box_part(id, [10., 10., 10.], [x, 0., 0.])}),
        );
    }
    let feature = json!({"id":"extra","geometry":{"op":"box","size":[1,1,1]}});
    let stale = [
        (
            "put_part",
            json!({"id":"d","part":box_part("d", [1., 1., 1.], [0., 0., 0.])}),
        ),
        (
            "put_parts",
            json!({"parts":{"d":box_part("d", [1., 1., 1.], [0., 0., 0.])}}),
        ),
        (
            "import_part",
            json!({"id":"d","name":"D","path":dir.path().join("d.stl").display().to_string()}),
        ),
        ("set_param", json!({"name":"width","expression":"10"})),
        ("add_feature", json!({"part":"a","feature":feature})),
        (
            "edit_feature",
            json!({"part":"a","id":"a","geometry":{"op":"box","size":[1,1,1]}}),
        ),
        ("delete_feature", json!({"part":"a","id":"a"})),
        ("move_feature", json!({"part":"a","id":"a"})),
        ("suppress_feature", json!({"part":"a","id":"a"})),
        ("set_part_info", json!({"id":"a","name":"A"})),
        ("move_part", json!({"id":"a","translate":[1.,0.,0.]})),
        (
            "align_parts",
            json!({"id":"a","target":"b","axes":["x"],"mode":"min"}),
        ),
        ("remove_part", json!({"id":"a"})),
        ("set_contact", json!({"a":"a","b":"b","allowed":true})),
        (
            "set_section",
            json!({"id":"mid","section":{"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}}),
        ),
        ("remove_section", json!({"id":"mid"})),
        ("set_motion", json!({})),
        ("batch", json!({"operations":[]})),
        ("restore", json!({"name":"none"})),
        ("undo", json!({})),
        ("redo", json!({})),
    ];
    for (tool, arguments) in stale {
        let mut arguments = arguments;
        arguments["expected_revision"] = json!(0);
        let text = s.refused(tool, arguments);
        assert!(text.contains("expected 0, current 3"), "{tool}: {text}");
    }
    s.finish();
}

#[test]
fn feature_info_reads_a_solid_and_a_plane() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok("put_part", json!({"id":"block","part":{"name":"Block","colour":[80,100,120],"geometry":{"op":"history","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
        {"id":"frame","geometry":{"op":"plane","placement":{"translation":[1,2,3],"rotation":[0,0,90]}}}
    ],"output":"base"}}}));
    let solid = s.ok("feature_info", json!({"part":"block","feature":"base"}));
    assert_eq!(solid["kind"], "box", "{solid}");
    assert!(
        (solid["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6,
        "{solid}"
    );
    assert_near(&solid["bounds_mm"], &[0., 0., 0., 10., 20., 30.]);
    assert_eq!(solid["faces"], 6);
    assert_eq!(solid["edges"], 12);
    assert_eq!(solid["suppressed"], false);
    assert_eq!(solid["depends_on"], json!([]));
    let plane = s.ok("feature_info", json!({"part":"block","feature":"frame"}));
    assert_eq!(plane["kind"], "plane", "{plane}");
    assert_near(&plane["origin"], &[1., 2., 3.]);
    assert_near(&plane["x"], &[0., 1., 0.]);
    assert_near(&plane["normal"], &[0., 0., 1.]);
    let unknown = s.refused("feature_info", json!({"part":"block","feature":"nope"}));
    assert!(unknown.contains("Unknown feature nope"), "{unknown}");
    s.finish();
}

#[test]
fn render_sends_no_image_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"box","part":box_part("Box", [10., 10., 10.], [0., 0., 0.])}),
    );
    let png = dir.path().join("view.png").display().to_string();
    let plain = s.rpc(
        "tools/call",
        json!({"name":"render","arguments":{"path":png,"width":128,"height":96}}),
    );
    assert_ne!(plain["isError"], true, "{plain}");
    assert_eq!(plain["content"].as_array().unwrap().len(), 1, "{plain}");
    let text: Value = serde_json::from_str(plain["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["path"], png);
    let inline = s.rpc(
        "tools/call",
        json!({"name":"render","arguments":{"path":png,"width":128,"height":96,"inline":true}}),
    );
    assert_eq!(inline["content"][1]["mimeType"], "image/png", "{inline}");
    s.finish();
}

const GENERATED_SOURCE: &str = "{ name: 'Generated', parts: { base: { name: 'Base', colour: [80, 100, 120], geometry: { op: 'box', size: [10, 20, 30] } } } }";

#[test]
fn generated_project_refuses_feature_edits_and_allows_moves() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("design.jsonnet"), GENERATED_SOURCE).unwrap();
    let mut s = Session::start(dir.path());
    let status = s.ok("status", json!({}));
    assert_eq!(status["source"], "design.jsonnet", "{status}");
    assert_eq!(status["part_count"], 1, "{status}");
    let message =
        "This project is generated from design.jsonnet. Edit the source, then call build.";
    let feature = json!({"id":"hole","geometry":{"op":"box","size":[1,1,1]}});
    let refused = s.refused("add_feature", json!({"part":"base","feature":feature}));
    assert_eq!(refused, message);
    let refused = s.refused("set_param", json!({"name":"width","expression":"5"}));
    assert_eq!(refused, message);
    let moved = s.ok("move_part", json!({"id":"base","translate":[5,0,0]}));
    assert_eq!(
        moved["changed"][0]["placement"]["translation"][0].as_f64(),
        Some(5.),
        "{moved}"
    );
    let status = s.ok("status", json!({}));
    assert_eq!(status["source"], "design.jsonnet", "{status}");
    s.finish();
}

#[test]
fn build_tool_reports_the_changes() {
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("design.jsonnet");
    std::fs::write(&design, GENERATED_SOURCE).unwrap();
    let mut s = Session::start(dir.path());
    let before = s.ok("status", json!({}))["revision"].as_u64().unwrap();
    let extra = "{ name: 'Generated', parts: { base: { name: 'Base', colour: [80, 100, 120], geometry: { op: 'box', size: [10, 20, 30] } }, extra: { name: 'Extra', colour: [1, 2, 3], geometry: { op: 'box', size: [5, 5, 5] } } } }";
    std::fs::write(&design, extra).unwrap();
    let report = s.ok("build", json!({}));
    assert_eq!(report["added_parts"], json!(["extra"]), "{report}");
    assert_eq!(report["removed_parts"], json!([]), "{report}");
    assert_eq!(report["parts"], 2, "{report}");
    assert!(
        report["written"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file == "parts/extra.json"),
        "{report}"
    );
    assert!(report["revision"].as_u64().unwrap() > before, "{report}");
    let status = s.ok("status", json!({}));
    assert_eq!(status["part_count"], 2, "{status}");
    std::fs::write(&design, GENERATED_SOURCE).unwrap();
    let report = s.ok("build", json!({}));
    assert_eq!(report["removed_parts"], json!(["extra"]), "{report}");
    let broken = "{ parts: { base: { name: 'Base', colour: [1, 1, 1], geometry: { op: 'box', size: [-1, 2, 3] } } } }";
    std::fs::write(&design, broken).unwrap();
    let refused = s.refused("build", json!({}));
    assert!(refused.contains("base"), "{refused}");
    assert_eq!(faber_files::read(dir.path()).unwrap().parts.len(), 1);
    let status = s.ok("status", json!({}));
    assert!(
        status["source_error"].as_str().unwrap().contains("base"),
        "{status}"
    );
    assert_eq!(status["part_count"], 1, "{status}");
    s.finish();
}

#[test]
fn generated_undo_builds_the_source_again() {
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("design.jsonnet");
    std::fs::write(&design, GENERATED_SOURCE).unwrap();
    let mut s = Session::start(dir.path());
    s.ok("move_part", json!({"id":"base","translate":[5,0,0]}));
    let taller = "{ name: 'Generated', parts: { base: { name: 'Base', colour: [80, 100, 120], geometry: { op: 'box', size: [10, 20, 40] } } } }";
    std::fs::write(&design, taller).unwrap();
    s.ok("build", json!({}));
    s.ok("undo", json!({}));
    s.ok("undo", json!({}));
    let info = s.ok("part_info", json!({"id":"base"}));
    assert_eq!(info["recipe"]["geometry"]["size"][2], 40., "{info}");
    assert_eq!(info["recipe"]["placement"]["translation"][0], 0., "{info}");
    assert_eq!(info["recipe"]["placement"]["translation"][1], 0., "{info}");
    s.finish();
}

#[test]
fn set_motion_refuses_an_unknown_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    let text = s.refused("set_motion", json!({"bogus":1}));
    assert!(text.contains("bogus"), "{text}");
    s.finish();
}

/// A block with a fillet, a spare box, an axis and a point. The output is a plain box.
fn suppress_block() -> Value {
    json!({"name":"Suppress","colour":[100,140,170],"geometry":{"op":"history","output":"out","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
        {"id":"round","geometry":{"op":"fillet","source":"base","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":1}},
        {"id":"extra","geometry":{"op":"box","size":[1,1,1]}},
        {"id":"ax","geometry":{"op":"axis","origin":[1,2,3],"direction":[0,0,2]}},
        {"id":"pt","geometry":{"op":"point","position":[4,5,6]}},
        {"id":"out","geometry":{"op":"box","size":[10,20,30]}}
    ]}})
}

/// Imports a purchased screw. The source file is copied into the test directory.
fn import_screw(s: &mut Session, dir: &std::path::Path) {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/fastener-check/assets")
        .join("f940c8dadc7ad6fe0337a3802cafe12cc716e64c1c519f432a01dea892f6c25e");
    let step = dir.join("screw.step");
    std::fs::copy(source, &step).unwrap();
    s.ok(
        "import_part",
        json!({
            "id": "screw",
            "name": "Screw",
            "path": step.display().to_string(),
            "purchased": {"source": "FreeCAD-library", "licence": "CC BY 3.0", "designation": "M3 x 10"}
        }),
    );
}

fn ids_of(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|part| part["id"].as_str().unwrap())
        .collect()
}

#[test]
fn batch_places_and_suppresses_from_json() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"base","part":box_part("Base", [40., 40., 10.], [0., 0., 0.])}),
    );
    s.ok(
        "put_part",
        json!({"id":"top","part":box_part("Top", [10., 10., 5.], [0., 0., 20.])}),
    );
    s.ok("put_part", json!({"id":"block","part":suppress_block()}));
    let preview = s.ok(
        "preview_batch",
        json!({"operations":[
            {"tool":"move_part","id":"top","translate":[5.,0.,0.]}
        ]}),
    );
    let candidate = preview["candidate"]["parts"].as_array().unwrap();
    let moved = candidate.iter().find(|part| part["id"] == "top").unwrap();
    assert_near(&moved["bounds_mm"], &[5., 0., 20., 15., 10., 25.]);
    let still = s.ok("part_info", json!({"id":"top"}));
    assert_near(&still["bounds_mm"], &[0., 0., 20., 10., 10., 25.]);

    let reply = s.ok("batch", json!({"operations":[
        {"tool":"move_part","id":"top","translate":[0.,0.,1.]},
        {"tool":"align_parts","id":"top","target":"base","axes":["z"],"mode":"min_to_max","gap_mm":2.},
        {"tool":"suppress_feature","part":"block","id":"round"}
    ]}));
    assert_eq!(ids_of(&reply["changed"]), ["block", "top"], "{reply}");
    let top = s.ok("part_info", json!({"id":"top"}));
    assert_near(&top["bounds_mm"], &[0., 0., 12., 10., 10., 17.]);
    let feature = s.ok("feature_info", json!({"part":"block","feature":"round"}));
    assert_eq!(feature["suppressed"], true, "{feature}");
    s.finish();
}

#[test]
fn batch_refuses_the_whole_change_when_a_placement_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"top","part":box_part("Top", [10., 10., 5.], [0., 0., 20.])}),
    );
    let before = s.ok("part_info", json!({"id":"top"}));
    let refused = s.refused(
        "batch",
        json!({"operations":[
            {"tool":"move_part","id":"top","translate":[5.,0.,0.]},
            {"tool":"align_parts","id":"top","target":"nowhere","axes":["z"],"mode":"min"}
        ]}),
    );
    assert!(refused.starts_with("Batch operation 2: "), "{refused}");
    assert!(refused.contains("nowhere"), "{refused}");
    let after = s.ok("part_info", json!({"id":"top"}));
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(after["recipe"]["placement"], before["recipe"]["placement"]);
    s.finish();
}

#[test]
fn batch_move_copy_of_a_purchased_part_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    import_screw(&mut s, dir.path());
    let refused = s.refused(
        "batch",
        json!({"operations":[
            {"tool":"move_part","id":"screw","translate":[1.,0.,0.],"copy_as":"copy"}
        ]}),
    );
    assert_eq!(
        refused,
        "Part screw is a purchased part. A copy is not permitted."
    );
    s.finish();
}

#[test]
fn part_info_lists_feature_ids_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok("put_part", json!({"id":"block","part":suppress_block()}));
    let ids = s.ok("part_info", json!({"id":"block","features":"ids"}));
    assert_eq!(
        ids["recipe"]["geometry"]["features"],
        json!(["base", "round", "extra", "ax", "pt", "out"])
    );
    let full = s.ok("part_info", json!({"id":"block"}));
    assert_eq!(full["recipe"]["geometry"]["features"][0]["id"], "base");
    let bad = s.refused("part_info", json!({"id":"block","features":"all"}));
    assert!(bad.contains("ids"), "{bad}");
    s.finish();
}

#[test]
fn feature_info_reads_suppressed_and_reference_features() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok("put_part", json!({"id":"block","part":suppress_block()}));
    s.ok("suppress_feature", json!({"part":"block","id":"round"}));
    s.ok("suppress_feature", json!({"part":"block","id":"extra"}));
    let modifier = s.ok("feature_info", json!({"part":"block","feature":"round"}));
    assert_eq!(modifier["kind"], "fillet", "{modifier}");
    assert_eq!(modifier["suppressed"], true, "{modifier}");
    assert!(
        (modifier["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6,
        "{modifier}"
    );
    let creation = s.ok("feature_info", json!({"part":"block","feature":"extra"}));
    assert_eq!(creation["kind"], "box", "{creation}");
    assert_eq!(creation["suppressed"], true, "{creation}");
    assert!(creation.get("volume_mm3").is_none(), "{creation}");
    let axis = s.ok("feature_info", json!({"part":"block","feature":"ax"}));
    assert_eq!(axis["kind"], "axis", "{axis}");
    assert_near(&axis["origin"], &[1., 2., 3.]);
    assert_near(&axis["direction"], &[0., 0., 2.]);
    let point = s.ok("feature_info", json!({"part":"block","feature":"pt"}));
    assert_eq!(point["kind"], "point", "{point}");
    assert_near(&point["point"], &[4., 5., 6.]);
    s.finish();
}

#[test]
fn feature_info_refuses_a_purchased_part() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    import_screw(&mut s, dir.path());
    let refused = s.refused("feature_info", json!({"part":"screw","feature":"base"}));
    assert_eq!(refused, "Part screw has no feature history");
    s.finish();
}

#[test]
fn set_param_lists_only_the_parts_that_follow_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok("set_param", json!({"name":"w","expression":"10"}));
    s.ok("put_part", json!({"id":"bar","part":{"name":"Bar","colour":[100,140,170],"geometry":{"op":"box","size":["w",10,10]}}}));
    s.ok(
        "put_part",
        json!({"id":"fixed","part":box_part("Fixed", [5., 5., 5.], [50., 0., 0.])}),
    );
    let reply = s.ok("set_param", json!({"name":"w","expression":"20"}));
    assert_eq!(reply["param"]["value"], 20., "{reply}");
    assert_eq!(ids_of(&reply["changed"]), ["bar"], "{reply}");
    s.finish();
}

#[test]
fn move_and_align_reply_lists_the_moved_part_and_set_motion_lists_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    s.ok(
        "put_part",
        json!({"id":"base","part":box_part("Base", [40., 40., 10.], [0., 0., 0.])}),
    );
    s.ok(
        "put_part",
        json!({"id":"top","part":box_part("Top", [10., 10., 5.], [0., 0., 20.])}),
    );
    let moved = s.ok("move_part", json!({"id":"top","translate":[1.,0.,0.]}));
    assert_eq!(moved["part_count"], 2, "{moved}");
    assert_eq!(ids_of(&moved["changed"]), ["top"], "{moved}");
    let aligned = s.ok(
        "align_parts",
        json!({"id":"top","target":"base","axes":["z"],"mode":"min_to_max"}),
    );
    assert_eq!(ids_of(&aligned["changed"]), ["top"], "{aligned}");
    assert!(aligned.get("bounds_mm").is_none(), "{aligned}");
    let motion = s.ok(
        "set_motion",
        json!({"expected_revision": aligned["revision"]}),
    );
    assert_eq!(motion["changed"], json!([]), "{motion}");
    assert_eq!(motion["part_count"], 2, "{motion}");
    s.finish();
}

#[test]
fn set_motion_still_refuses_an_unknown_field_with_verbose() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(dir.path());
    let text = s.refused("set_motion", json!({"verbose":true,"bogus":1}));
    assert!(text.contains("bogus"), "{text}");
    s.finish();
}
