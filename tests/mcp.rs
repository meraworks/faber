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
    let before = std::fs::read(dir.path().join("faber.json")).unwrap();
    let base_revision = serde_json::from_slice::<Value>(&before).unwrap()["revision"]
        .as_u64()
        .unwrap();
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
                "path":dir.path().join(format!("{view}.png")),"width":128,"height":96,"view":view
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
        std::fs::read(dir.path().join("faber.json")).unwrap(),
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
        std::fs::read(dir.path().join("faber.json")).unwrap(),
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
        std::fs::read(dir.path().join("faber.json")).unwrap(),
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
        json!({"name":"put_part","arguments":{"id":"base","part":expression_part}}),
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
        json!({"name":"batch","arguments":{"operations":[
            {"tool":"set_param","name":"height","expression":"60"},
            {"tool":"edit_feature","part":"base","id":"box","geometry":{"op":"box","size":[10,20,"height"]}}
        ]}}),
    );
    assert_ne!(changed["isError"], true, "{changed}");
    let report: Value =
        serde_json::from_str(changed["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!((report["parts"][0]["volume_mm3"].as_f64().unwrap() - 12000.).abs() < 1e-6);
    let undone = request("tools/call", json!({"name":"undo","arguments":{}}));
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
    let before = std::fs::read(dir.path().join("faber.json")).unwrap();
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
        std::fs::read(dir.path().join("faber.json")).unwrap(),
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
    let saved = std::fs::read(dir.path().join("faber.json")).unwrap();
    let bad_path = dir.path().join("bad.step");
    std::fs::write(&bad_path, b"not a model").unwrap();
    let refused = request(
        "tools/call",
        json!({"name":"import_part","arguments":{
            "id":"bad","name":"Bad model","path":bad_path
        }}),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(std::fs::read(dir.path().join("faber.json")).unwrap(), saved);
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
