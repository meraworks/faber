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
fn schema_tool_gives_one_op() {
    let mut session = Session::start();
    let value = session.ok("schema", json!({"kind":"geometry","op":"pipe"}));
    let text = value.to_string();
    assert!(text.contains("\"diameter\""), "{text}");
    assert!(text.contains("\"wall\""), "{text}");
    assert!(text.len() < 4_000, "{} characters", text.len());

    let error = session.refused("schema", json!({"kind":"geometry","op":"boxx"}));
    assert!(
        error.starts_with("Unknown op boxx. Nearest: box."),
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
