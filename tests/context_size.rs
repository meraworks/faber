//! Checks the size of the MCP context, the schema tool, the guide topics and typed errors.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
    init: Value,
}

impl Session {
    fn start() -> Self {
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
            init: Value::Null,
        };
        session.init = session.rpc(
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
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn open_project(session: &mut Session, dir: &Path) {
    session.ok(
        "open",
        json!({"path":dir.display().to_string(),"create":true,"name":"Context"}),
    );
}

#[test]
fn form_info_is_small_without_detail() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Session::start();
    open_project(&mut client, dir.path());
    let made=client.ok("new_form",json!({"id":"ring","primitive":"torus","major":12,"minor":3,"sides":16,"rings":8,"levels":1,"expected_revision":0}));
    let info = client.ok("form_info", json!({"part":"ring"}));
    assert_eq!(info["euler"], 0);
    assert_eq!(info["mesh"]["watertight"], true);
    assert!(info.get("cage").is_none() && info.to_string().len() < 1500);
    let full = client.ok("form_info", json!({"part":"ring","detail":true}));
    assert_eq!(full["cage"]["vertices"].as_array().unwrap().len(), 128);
    let op = json!({"op":"move","select":{"vertices":[0]},"by":[0,0,0.25]});
    assert!(
        client
            .refused(
                "edit_form",
                json!({"part":"ring","ops":[op],"expected_revision":0})
            )
            .contains("revision")
    );
    let edited = client.ok(
        "edit_form",
        json!({"part":"ring","ops":[op],"expected_revision":made["revision"]}),
    );
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["changed"]["id"], "ring");
    assert!(
        client
            .refused("new_form", json!({"id":"ring","primitive":"box"}))
            .contains("new valid part id")
    );
    assert!(
        client
            .refused("new_form", json!({"id":"bad","primitive":"box","radius":3}))
            .contains("radius")
    );
}

#[test]
fn named_view_and_animation_follow_the_saved_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start();
    open_project(&mut s, dir.path());
    s.ok("put_part",json!({"id":"block","part":{"name":"Block","colour":[26,141,160],"geometry":{"op":"box","size":[3,4,10]}}}));
    let view = json!({"camera":{"eye":[14,-12,15],"target":[1,2,4],"up":[0,0,1],"ortho_height":18},"hidden":["removed"]});
    s.refused(
        "set_view",
        json!({"name":"side","view":view,"expected_revision":0}),
    );
    s.ok(
        "set_view",
        json!({"name":"side","view":view,"expected_revision":1}),
    );
    let file = dir.path().join("side.png");
    let manifest = dir.path().join("faber.json");
    let saved = std::fs::read(&manifest).unwrap();
    s.refused(
        "render",
        json!({"view":"side","path":manifest,"quality":"high","samples":1,"width":64,"height":64}),
    );
    assert_eq!(std::fs::read(manifest).unwrap(), saved);
    let picture = s.ok(
        "render",
        json!({"view":"side","path":file,"quality":"high","samples":1,"width":64,"height":64}),
    );
    assert_eq!(
        picture["warnings"],
        json!(["View side: part removed was not found."])
    );
    assert_eq!(picture["samples_done"], 1);
    assert!(file.is_file());
    let out = dir.path().join("side.apng");
    s.ok("animate",json!({"kind":"turntable","view":"side","frames":3,"fps":12,"width":64,"height":64,"path":out}));
    assert!(out.is_file());
    s.ok("remove_view", json!({"name":"side","expected_revision":2}));
    s.refused("render", json!({"view":"side","path":file}));
    s.ok("undo", json!({}));
    assert!(
        s.ok("schema", json!({"kind":"view"}))["definition"]["properties"]["camera"].is_object()
    );
    assert!(
        !s.ok(
            "render",
            json!({"view":"side","width":64,"height":64,"path":file})
        )
        .is_null()
    );
    s.ok("set_view", json!({"name":"Ön görünüş","view":view}));
    assert_eq!(
        s.ok(
            "render",
            json!({"view":"Ön görünüş","width":64,"height":64,"path":file})
        )["view"],
        "Ön görünüş"
    );
}

#[test]
fn set_operation_then_machining_lists_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start();
    open_project(&mut s, dir.path());
    s.ok("put_part",json!({"id":"plate","part":{"name":"Plate","colour":[80,140,160],"geometry":{"op":"box","size":[40,20,5]}}}));
    let tool = json!({"kind":"flat","diameter":4,"flute_length":16,"flutes":2,"feed_mm_min":240,"plunge_mm_min":60,"spindle_rpm":8000,"number":1});
    let setup = json!({"part":"plate","up":[0,0,1],"stock":{"kind":"offset","sides":0,"top":1,"bottom":0},"origin":"stock_top_corner"});
    let op = json!({"id":"face_top","kind":"face","tool":"t1","setup":"mill","stepover":2,"stepdown":1,"heights":{"clearance":5,"retract":3,"top":0,"bottom":-1}});
    let batch = json!({"operations":[{"tool":"set_tool","id":"t1","definition":tool},{"tool":"set_setup","id":"mill","setup":setup},{"tool":"set_operation","operation":op}]});
    let mut bad = batch.clone();
    bad["operations"][2]["operation"]["tool"] = json!("absent");
    assert!(s.refused("batch", bad).contains("absent"));
    assert!(!dir.path().join("manufacture.json").exists());
    s.ok("batch", batch);
    let mut wider = tool.clone();
    wider["diameter"] = json!(8);
    let revision = s.ok("set_tool", json!({"id":"wider","definition":wider}))["revision"]
        .as_u64()
        .unwrap();
    let mut second = op.clone();
    second["id"] = json!("second");
    second["tool"] = json!("wider");
    let duplicate = s.refused("set_operation", json!({"operation":second}));
    assert!(duplicate.contains("number 1"), "{duplicate}");
    let mut twist_drill = tool.clone();
    twist_drill["kind"] = json!("drill");
    assert!(
        s.refused("set_tool", json!({"id":"t1","definition":twist_drill}))
            .contains("drill")
    );
    let mut unknown = op.clone();
    unknown["setup"] = json!("missing_setup");
    assert!(
        s.refused("set_operation", json!({"operation":unknown}))
            .contains("missing_setup")
    );
    s.refused(
        "remove_operation",
        json!({"id":"face_top","expected_revision":revision-1}),
    );
    let list = s.ok("machining", json!({}));
    assert_eq!(list["operations"][0]["id"], "face_top");
    s.ok(
        "remove_operation",
        json!({"id":"face_top","expected_revision":revision}),
    );
    assert_eq!(s.ok("machining", json!({}))["operations"], json!([]));
    s.ok("undo", json!({}));
    let report = s.ok("toolpaths", json!({"setup":"mill"}));
    assert!(report.to_string().len() < 3500, "{report}");
    assert_eq!(report["operations"][0]["gouge_mm"], 0.);
    drop(s);
    let mut reopened = Session::start();
    reopened.ok("open", json!({"path":dir.path()}));
    assert_eq!(
        reopened.ok("machining", json!({}))["operations"],
        list["operations"]
    );
    let mcp_file = dir.path().join("from-mcp.nc");
    reopened.ok(
        "write_gcode",
        json!({"setup":"mill","dialect":"grbl","path":mcp_file}),
    );
    let cli_file = dir.path().join("from-cli.nc");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("gcode")
        .arg(dir.path())
        .arg("mill")
        .arg(&cli_file)
        .args(["--dialect", "grbl"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(cli_file).unwrap(),
        std::fs::read(&mcp_file).unwrap()
    );
    let before = std::fs::read(&mcp_file).unwrap();
    std::fs::write(
        dir.path().join("design.jsonnet"),
        "error 'invalid new plate'",
    )
    .unwrap();
    assert!(
        reopened
            .refused(
                "write_gcode",
                json!({"setup":"mill","path":mcp_file,"force":true})
            )
            .contains("invalid new plate")
    );
    let error = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("gcode")
        .arg(dir.path())
        .arg("mill")
        .arg(&mcp_file)
        .arg("--force")
        .output()
        .unwrap();
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("invalid new plate"));
    assert_eq!(std::fs::read(mcp_file).unwrap(), before);
}

#[test]
fn feature_info_of_a_surface_has_no_volume_and_retains_the_stitch_gap_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::start();
    open_project(&mut session, dir.path());
    let square = |id: &str, x: f64| {
        json!({"id":id,"geometry":{"op":"sketch","profile":{
            "outer":{"kind":"rectangle","origin":[x,0],"size":[20,20]}
        }}})
    };
    let part = json!({"name":"Joined panels","colour":[40,90,110],"geometry":{"op":"history","features":[
        square("a",0.),square("b",20.01),
        {"id":"join","geometry":{"op":"stitch","surfaces":["a","b"],"tolerance":0.02}},
        {"id":"stock","geometry":{"op":"box","size":[20,40,10]}}
    ],"output":"stock"}});
    session.ok("put_part", json!({"id":"panels","part":part}));
    let args = json!({"part":"panels","feature":"join"});
    let first = session.ok("feature_info", args.clone());
    assert_eq!(first["kind"], "surface");
    assert_eq!(first["operation"], "stitch");
    assert_eq!(first["open_edges"], 6);
    assert!(first.get("volume_mm3").is_none());
    assert!((first["largest_gap_mm"].as_f64().unwrap() - 0.01).abs() < 1e-6);
    session.ok("put_part",json!({"id":"copy","part":{"name":"Other panels","colour":[32,80,80],
        "geometry":{"op":"instance","of":"panels"},"placement":{"translation":[0,20,3],"rotation":[0,0,90]}}}));
    let copy = session.ok("feature_info", json!({"part":"copy","feature":"join"}));
    assert_eq!(copy["area_mm2"], first["area_mm2"]);
    assert_eq!(copy["bounds_mm"], first["bounds_mm"]);
    let first = session.ok("feature_info", args.clone());
    drop(session);
    let mut reopened = Session::start();
    reopened.ok("open", json!({"path":dir.path()}));
    assert_eq!(reopened.ok("feature_info", args), first);
}

#[test]
fn flat_pattern_reads_a_sheet_and_exports_from_the_cli_and_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start();
    open_project(&mut s, dir.path());
    let angle = json!({"name":"Bracket","colour":[40,90,110],"geometry":{
        "op":"sheet_metal","rule":{"thickness":2,"bend_radius":2},
        "base":{"outer":{"kind":"rectangle","origin":[0,0],"size":[50,40]}},
        "flanges":[{"id":"side","on":"base","edge":2,"length":30}]}});
    s.ok("put_part", json!({"id":"bracket","part":angle}));
    let dxf = dir.path().join("cut.dxf");
    let report = s.ok("flat_pattern", json!({"part":"bracket","path":dxf}));
    assert!((report["size_mm"][0].as_f64().unwrap() - 84.52389342).abs() < 0.0001);
    assert_eq!(report["k_factor"], 0.44);
    assert_eq!(report["bends"][0]["direction"], "up");
    assert!(report["problems"].as_array().unwrap().is_empty());
    assert!(report.to_string().len() < 2500);
    let saved = std::fs::read_to_string(&dxf).unwrap();
    assert!(saved.lines().any(|line| line.trim() == "POLYLINE"));
    assert!(saved.lines().any(|line| line.trim() == "BEND"));
    drop(s);
    let svg = dir.path().join("cut.svg");
    let result = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("flat")
        .arg(dir.path())
        .arg("bracket")
        .arg(&svg)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let other: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(other["revision"], report["revision"]);
    assert_eq!(other["bends"], report["bends"]);
    assert!(std::fs::read_to_string(svg).unwrap().contains("BEND"));
    let invalid = dir.path().join("keep.txt");
    std::fs::write(&invalid, "keep").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("flat")
        .arg(dir.path())
        .arg("bracket")
        .arg(&invalid)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read_to_string(invalid).unwrap(), "keep");
}

#[test]
fn each_study_kind_replies_below_2500_characters() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start();
    open_project(&mut s, dir.path());
    s.ok("put_part",json!({"id":"bar","part":{"name":"Bar","material":"steel-s235","colour":[50,100,150],"geometry":{"op":"box","size":[20,8,8]}}}));
    let left = json!({"normal":[-1,0,0],"expect":1,"tolerance":0.001});
    let right = json!({"normal":[1,0,0],"expect":1,"tolerance":0.001});
    for (index, kind) in [
        json!({"kind":"modal","modes":3}),
        json!({"kind":"thermal","stress":true,"reference_c":20,"conditions":[
        {"kind":"temperature","celsius":-15,"face":left},{"kind":"power","watts":3,"face":right}]}),
        json!({"kind":"shape","target_volume":0.65}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut study = json!({"part":"bar","resolution_mm":2,"fixtures":[{"kind":"fixed","face":left}],"loads":[{"kind":"force","face":right,"newtons":[0,0,-15]}]});
        study
            .as_object_mut()
            .unwrap()
            .extend(kind.as_object().unwrap().clone());
        s.ok("set_study", json!({"id":"test","study":study}));
        let path = dir.path().join(format!("{index}.png"));
        let mut args = json!({"id":"test","render":{"path":path,"width":320,"height":240}});
        if index == 0 {
            args["render"]["mode"] = json!(2);
            args["render"]["deform_scale"] = json!(2);
        }
        let report = s.ok("run_study", args);
        assert_eq!(report["kind"], kind["kind"]);
        assert!(report.to_string().len() < 2500);
        assert_eq!(
            image::open(path).unwrap().into_rgb8().dimensions(),
            (320, 240)
        );
        if index == 2 {
            assert!(
                s.refused(
                    "run_study",
                    json!({"id":"test","save":true,"expected_revision":1})
                )
                .contains("revision")
            );
            assert_eq!(
                s.ok(
                    "run_study",
                    json!({"id":"test","save":true,"expected_revision":4})
                )["new_part"],
                "bar-shape"
            );
        }
    }
}

/// Operation names of the Geometry schema, read from the schemars schema.
fn schema_op_names() -> Vec<String> {
    let schema = serde_json::to_value(schemars::schema_for!(
        faber_model::Geometry<faber_model::Scalar>
    ))
    .unwrap();
    let variants = schema["oneOf"]
        .as_array()
        .or_else(|| schema["anyOf"].as_array())
        .expect("Geometry schema has variants");
    variants
        .iter()
        .map(|variant| {
            variant["properties"]["op"]["const"]
                .as_str()
                .expect("each variant names its op")
                .to_string()
        })
        .collect()
}

#[test]
fn tools_list_is_small() {
    let mut session = Session::start();
    let list = session.rpc("tools/list", json!({}));
    let total = serde_json::to_string(&list).unwrap().len();
    assert!(total < 60_000, "tools/list is {total} characters");
    for tool in list["tools"].as_array().unwrap() {
        let size = serde_json::to_string(tool).unwrap().len();
        assert!(size <= 6_000, "{} is {size} characters", tool["name"]);
    }
}

#[test]
fn instructions_are_short() {
    let session = Session::start();
    let text = session.init["instructions"].as_str().unwrap();
    assert!(text.chars().count() <= 2_500, "{} characters", text.len());
    assert!(text.contains("guide"), "{text}");
    assert!(text.contains("schema"), "{text}");
}

#[test]
fn surface_tools_report_areas_and_render_without_changing_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start();
    open_project(&mut s, dir.path());
    s.ok(
        "put_part",
        json!({"id":"block","part":{"name":"Block","colour":[32,90,110],
        "geometry":{"op":"box","size":[20,10,5]}}}),
    );
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let png = dir.path().join("curvature.png");
    let summary = s.ok(
        "curvature",
        json!({"part":"block","render":{"path":png,"mode":"gauss"}}),
    );
    assert!(summary.get("faces").is_none());
    assert!(summary["min_convex_radius_mm"].is_null());
    assert!(summary["min_concave_radius_mm"].is_null());
    assert_eq!(&std::fs::read(&png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    let detail = s.ok("curvature", json!({"part":"block","detail":true}));
    assert_eq!(detail["faces"].as_array().unwrap().len(), 6);
    let draft = s.ok(
        "draft_analysis",
        json!({"part":"block","pull":[0,0,1],"min_deg":2}),
    );
    assert!((draft["vertical_mm2"].as_f64().unwrap() - 300.).abs() < 1e-8);
    let access = s.ok(
        "access_analysis",
        json!({"part":"block","directions":[[0,0,1]],"scene":true}),
    );
    assert!((access["reachable_mm2"].as_f64().unwrap() - 200.).abs() < 1e-8);
    for report in [summary, draft, access] {
        assert!(report["method"].is_string() && !report["resolution"].is_null());
        assert!(!report["limits"].as_array().unwrap().is_empty());
    }
    assert!(
        s.refused("draft_analysis", json!({"part":"block","pull":[0,0,0]}))
            .contains("zero")
    );
    assert!(
        s.refused("curvature", json!({"part":"block","resolution":0.000001}))
            .contains("0.001")
    );
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}

#[test]
fn schema_tool_gives_one_op() {
    let mut session = Session::start();
    let value = session.ok("schema", json!({"kind":"geometry","op":"pipe"}));
    let text = value.to_string();
    assert!(text.contains("\"diameter\""), "{text}");
    assert!(text.contains("\"wall\""), "{text}");
    assert!(text.len() < 4_000, "{} characters", text.len());

    let error = session.refused("schema", json!({"kind":"geometry","op":"boxx"}));
    assert!(
        error.starts_with("Unknown op boxx. Nearest: box, boss."),
        "{error}"
    );
}

#[test]
fn op_list_names_every_op() {
    let mut session = Session::start();
    let value = session.ok("schema", json!({"kind":"geometry"}));
    let names: Vec<String> = value["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            line.as_str()
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    let mut expected = schema_op_names();
    let mut listed = names.clone();
    expected.sort();
    listed.sort();
    assert_eq!(listed, expected);
}

#[test]
fn typed_errors_survive() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::start();
    open_project(&mut session, dir.path());
    let bad_outer = json!([[0, 0], [10, 0], [10, true]]);
    let error = session.refused(
        "put_part",
        json!({"id":"base","part":{"name":"Base","geometry":{"op":"extrude","profile":{"outer":bad_outer},"height":5},"colour":[80,100,120]}}),
    );
    assert!(
        error.starts_with("failed to deserialize parameters: part.geometry.profile.outer[2]: "),
        "{error}"
    );
    let error = session.refused(
        "add_feature",
        json!({"part":"base","feature":{"id":"walls","geometry":{"op":"extrude","profile":{"outer":bad_outer},"height":5}}}),
    );
    assert!(
        error.starts_with("failed to deserialize parameters: feature.geometry.profile.outer[2]: "),
        "{error}"
    );
}

#[test]
fn guide_topic_returns_one_section() {
    let mut session = Session::start();
    let index = session.ok("guide", json!({}));
    let topics: Vec<&str> = index["topics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|topic| topic.as_str().unwrap())
        .collect();
    assert!(topics.contains(&"Quick start"), "{topics:?}");
    assert!(index["quick_start"].as_str().unwrap().contains("six steps"));

    let section = session.ok("guide", json!({"topic":"Quick start"}));
    let text = section["text"].as_str().unwrap();
    assert!(text.starts_with("## Quick start"), "{text}");
    assert!(text.contains("six steps"), "{text}");
    assert!(!text.contains("## Common mistakes"), "{text}");

    let error = session.refused("guide", json!({"topic":"Quik start"}));
    assert!(error.contains("Nearest: Quick start."), "{error}");
}

#[test]
fn unknown_material_names_the_nearest_and_material_in_use_is_not_removed() {
    let mut session = Session::start();
    let dir = tempfile::tempdir().unwrap();
    open_project(&mut session, dir.path());
    let part = json!({"name":"Cube","colour":[10,20,30],"geometry":{"op":"box","size":[10,10,10]}});
    session.ok("put_part", json!({"id":"cube","part":part}));
    assert_eq!(
        session.refused("set_part_info", json!({"id":"cube","material":"pett"})),
        "Unknown material pett. Nearest: petg."
    );
    let mut row = session.ok("materials", json!({"id":"pla"}));
    row.as_object_mut().unwrap().remove("id");
    row["density_g_cm3"] = json!(2.0);
    session.ok(
        "set_material",
        json!({"id":"custom","material":row,"expected_revision":1}),
    );
    assert!(
        session
            .refused(
                "set_part_info",
                json!({"id":"cube","material":"custom","expected_revision":1})
            )
            .contains("revision")
    );
    session.ok(
        "set_part_info",
        json!({"id":"cube","material":"custom","expected_revision":2}),
    );
    let mass = session.ok("mass_properties", json!({"id":"cube"}));
    assert_eq!(mass["material"], "custom");
    assert_eq!(mass["density_source"], "material");
    assert_eq!(mass["density_g_cm3"], 2.);
    let overridden = session.ok("mass_properties", json!({"id":"cube","density_g_cm3":3.}));
    assert_eq!(overridden["density_source"], "request");
    assert_eq!(overridden["density_g_cm3"], 3.);
    assert_eq!(overridden["material"], "custom");
    assert!((mass["mass_g"].as_f64().unwrap() - 2.).abs() < 1e-9);
    let list = session.ok("materials", json!({"query":"custom"}));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], "custom");
    assert!(
        session
            .refused(
                "remove_material",
                json!({"id":"custom","expected_revision":3})
            )
            .contains("cube")
    );
    session.ok(
        "set_part_info",
        json!({"id":"cube","material":"","expected_revision":3}),
    );
    session.ok(
        "remove_material",
        json!({"id":"custom","expected_revision":4}),
    );
}
#[test]
fn edit_schema_exposes_the_material_assignment() {
    let mut session = Session::start();
    let reply = session.ok("schema", json!({"kind":"edit"}));
    let root = &reply["definition"];
    fn resolve<'a>(node: &'a Value, root: &'a Value) -> &'a Value {
        if let Some(reference) = node["$ref"].as_str() {
            root.pointer(reference.strip_prefix('#').unwrap())
                .expect("Broken edit schema reference")
        } else {
            node
        }
    }
    let mut pending = vec![root];
    let mut found = false;
    while let Some(node) = pending.pop() {
        let node = resolve(node, root);
        if node.pointer("/properties/tool/const") == Some(&json!("set_part_info")) {
            assert!(node["properties"].get("material").is_some());
            assert!(node["required"].as_array().unwrap().contains(&json!("id")));
            found = true;
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(children) = node[key].as_array() {
                pending.extend(children);
            }
        }
    }
    assert!(found, "The edit schema omits set_part_info.");
}

#[test]
fn sketch_dry_run_resolves_expressions_without_saving() {
    let mut session = Session::start();
    let mut args = json!({"sketch":{"points":{"p":["2 + 3",0]},"entities":{"rim":{"kind":"circle","centre":"p","radius":"3 * 2"}},"constraints":[{"kind":"radius","entity":"rim","value":"12 / 2"}]}});
    let solved = session.ok("solve_sketch", args.clone());
    assert_eq!(solved["points"]["p"], json!([5., 0.]));
    assert_eq!(solved["radii"]["rim"], 6.);
    args["sketch"]["constraints"][0]["value"] = json!("width / 2");
    assert!(
        session
            .refused("solve_sketch", args.clone())
            .contains("Unknown parameter: width")
    );
    let dir = tempfile::tempdir().unwrap();
    open_project(&mut session, dir.path());
    session.ok("set_param", json!({"name":"width","expression":"24"}));
    let manifest = dir.path().join("faber.json");
    let bytes = std::fs::read(&manifest).unwrap();
    let radius = session.ok("solve_sketch", args.clone())["radii"]["rim"]
        .as_f64()
        .unwrap();
    assert!((radius - 12.).abs() < 1e-9);
    args["sketch"]["constraints"][0]["value"] = json!("missing_width");
    assert!(
        session
            .refused("solve_sketch", args)
            .contains("Unknown parameter: missing_width")
    );
    assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
    assert_eq!(session.ok("status", json!({}))["revision"], 1);
}

#[test]
fn solve_sketch_reports_free_points_and_edits_check_the_revision() {
    let mut session = Session::start();
    let sketch =
        json!({"points":{"p":[0,0]},"entities":{"rim":{"kind":"circle","centre":"p","radius":5}}});
    let report = session.ok("solve_sketch", json!({"sketch":sketch}));
    assert_eq!(report["status"], "under");
    assert_eq!(report["dof"], 3);
    assert_eq!(report["free_points"], json!(["p"]));
    let dir = tempfile::tempdir().unwrap();
    open_project(&mut session, dir.path());
    session.ok(
        "put_part",
        json!({"id":"plate","part":{"name":"Plate","colour":[80,100,150],
        "geometry":{"op":"history","output":"pad","features":[
        {"id":"work","geometry":{"op":"plane","placement":{}}},
        {"id":"outline","geometry":{"op":"solved_sketch","plane":"work","sketch":sketch}},
        {"id":"pad","geometry":{"op":"extrude_sketch","source":"outline","vector":[0,0,10]}}]}}}),
    );
    let args = json!({"part":"plate","feature":"outline","expected_revision":1,
        "ops":[{"op":"add_constraint","constraint":{"kind":"radius","entity":"rim","value":7}}]});
    let changed = session.ok("edit_sketch", args.clone());
    assert_eq!(changed["revision"], 2);
    assert_eq!(changed["sketch_warnings"]["plate"]["outline"], 2);
    assert!(session.refused("edit_sketch", args).contains("revision"));
    assert_eq!(
        session.ok("status", json!({}))["sketch_warnings"]["plate"]["outline"],
        2
    );
}

#[test]
fn press_pull_uses_the_picked_face_and_checks_the_revision() {
    let mut session = Session::start();
    let dir = tempfile::tempdir().unwrap();
    open_project(&mut session, dir.path());
    session.ok("put_part",json!({"id":"cube","part":{"name":"Cube","colour":[75,110,190],"geometry":{"op":"box","size":[20,20,20]}}}));
    let answer = session.ok(
        "press_pull",
        json!({"id":"cube","face":1,"distance_mm":3.0,"expected_revision":1}),
    );
    assert_eq!(answer["revision"], 2);
    assert!((answer["volume_mm3"].as_f64().unwrap() - 9200.).abs() < 1e-6);
    assert!(
        session
            .refused(
                "press_pull",
                json!({"id":"cube","face":1,"distance_mm":3,"expected_revision":1})
            )
            .contains("revision")
    );
}

#[test]
fn quick_stress_replies_small_and_run_study_writes_the_picture() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::start();
    open_project(&mut session, dir.path());
    session.ok("put_part",json!({"id":"bar","part":{"name":"Bar","material":"aluminium-6061-t6","colour":[50,100,150],"geometry":{"op":"box","size":[20,4,4]}}}));
    let fixed = json!({"normal":[-1,0,0],"expect":1,"tolerance":0.0001});
    let end = json!({"normal":[1,0,0],"expect":1,"tolerance":0.0001});
    let load = json!({"face":end,"newtons":[0,0,-1]});
    let result = session.ok(
        "quick_stress",
        json!({"part":"bar","fixed":fixed,"force":load,"resolution_mm":1}),
    );
    assert!(result.to_string().len() < 2000);
    assert!(!result["limits"].as_array().unwrap().is_empty());
    assert!((result["reaction_n"][2].as_f64().unwrap() - 1.).abs() < 0.01);
    let study = json!({"part":"bar","fixtures":[{"kind":"fixed","face":fixed}],"loads":[{"kind":"force","face":end,"newtons":[0,0,-1]}],"resolution_mm":1});
    assert!(
        session
            .refused(
                "set_study",
                json!({"id":"bend","study":study,"expected_revision":0})
            )
            .contains("revision")
    );
    let mut bad = study.clone();
    bad["resolution_mm"] = json!(-1);
    assert!(
        session
            .refused(
                "set_study",
                json!({"id":"bend","study":bad,"expected_revision":1})
            )
            .contains("resolution")
    );
    assert!(!dir.path().join("analysis.json").exists());
    session.ok(
        "set_study",
        json!({"id":"bend","study":study,"expected_revision":1}),
    );
    let manifest = dir.path().join("faber.json");
    let bytes = std::fs::read(&manifest).unwrap();
    assert!(
        session
            .refused("run_study", json!({"id":"bend","render":{"path":manifest}}))
            .contains("PNG")
    );
    assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
    let path = dir.path().join("field.png");
    let saved = session.ok(
        "run_study",
        json!({"id":"bend","render":{"path":path,"width":360,"height":240,"deform_scale":3}}),
    );
    assert_eq!(result["max_von_mises_mpa"], saved["max_von_mises_mpa"]);
    let image = image::open(path).unwrap().into_rgb8();
    assert_eq!(image.dimensions(), (360, 240));
    let (mut red, mut blue) = (0, 0);
    for p in image.pixels() {
        if p[0] > 130 && p[0] / 2 > p[2] {
            red += 1;
        }
        if p[2] > 130 && p[2] / 2 > p[0] {
            blue += 1;
        }
    }
    assert!(
        red >= 20 && blue >= 20,
        "Need red and blue on the stress map: {red}, {blue}."
    );
    session.ok("remove_study", json!({"id":"bend","expected_revision":2}));
}

#[test]
fn viewer_tool_sends_display_actions_to_a_running_viewer() {
    use faber_wiring::viewer_link;
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::start();
    session.ok(
        "open",
        json!({"path":dir.path(),"create":true,"name":"Link"}),
    );
    session.ok(
        "put_part",
        json!({"id":"roll","part":{"name":"Roll","colour":[200,200,200],"placement":{"translation":[0,0,0],"rotation":[0,0,0]},"geometry":{"op":"box","size":[10,10,10]}}}),
    );
    let error = session.refused("viewer", json!({"action":"hide","parts":["missing"]}));
    assert!(error.contains("Part missing was not found."), "{error}");
    let reply = session.ok("viewer", json!({"action":"show_all"}));
    assert_eq!(reply["viewer"], false, "{reply}");

    // A stand-in viewer applies each command and reports the hidden parts.
    let root = dir.path().to_owned();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let running = stop.clone();
    let viewer = std::thread::spawn(move || {
        while !running.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(command) = viewer_link::read_command(&root) {
                let mut state = viewer_link::read_state(&root).unwrap_or_default();
                if command.seq > state.applied {
                    for action in command.actions {
                        if let viewer_link::ViewerAction::Hide { parts } = action {
                            state.hidden.extend(parts);
                        }
                    }
                    state.applied = command.seq;
                    viewer_link::write_state(&root, &state).unwrap();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let reply = session.ok("viewer", json!({"action":"hide","parts":["roll"]}));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    viewer.join().unwrap();
    assert_eq!(reply["viewer"], true, "{reply}");
    assert_eq!(reply["hidden"], json!(["roll"]));
    let state = session.ok("viewer", json!({}));
    assert_eq!(state["hidden"], json!(["roll"]));
    assert!(reply.to_string().len() < 400);
}
