//! Stdio batch errors name the operation index and the bad field or tool.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl Server {
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
        let mut server = Self {
            child,
            input,
            output,
            id: 0,
        };
        server.request(
            "initialize",
            json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
        );
        writeln!(
            server.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        server.input.flush().unwrap();
        server
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
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

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts a server and opens a new project in the directory.
fn open_project(dir: &std::path::Path) -> Server {
    let mut server = Server::start();
    let open = server.call(
        "open",
        json!({"path":dir,"create":true,"name":"Batch errors"}),
    );
    assert_ne!(open["isError"], true, "{open}");
    server
}

/// Sends a batch and returns the error text. The call must fail.
fn refused_text(server: &mut Server, operations: Value) -> String {
    let reply = server.call("batch", json!({"operations": operations}));
    assert_eq!(reply["isError"], true, "{reply}");
    reply["content"][0]["text"].as_str().unwrap().to_string()
}

#[test]
fn unknown_tool_names_the_operation_and_the_accepted_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"set_param","name":"width","expression":"10"},
            {"tool":"put_prat","id":"box","part":{}}
        ]),
    );
    assert!(text.contains("Batch operation 2"), "{text}");
    assert!(text.contains("put_prat"), "{text}");
    assert!(text.contains("set_part_info"), "{text}");
    assert!(!text.contains("untagged"), "{text}");
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}

#[test]
fn misspelled_part_field_names_the_operation_and_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"set_param","name":"width","expression":"10"},
            {"tool":"put_part","id":"base","part":{
                "name":"Base",
                "geometry":{"op":"box","size":[10,20,30]},
                "colr":[80,100,120]
            }}
        ]),
    );
    assert!(text.contains("Batch operation 2"), "{text}");
    assert!(text.contains("colr"), "{text}");
    assert!(!text.contains("untagged"), "{text}");
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}

#[test]
fn wrong_colour_type_in_set_part_info_names_the_operation_and_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"set_param","name":"width","expression":"10"},
            {"tool":"set_part_info","id":"box","colour":"red"}
        ]),
    );
    assert!(text.contains("Batch operation 2"), "{text}");
    assert!(text.contains("colour"), "{text}");
    assert!(!text.contains("untagged"), "{text}");
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}

#[test]
fn value_errors_name_the_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let text = refused_text(
        &mut server,
        json!([{"tool":"put_part","id":"label","part":{"name":"Label","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
            {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"text","text":"Hi","height":-5,"at":[0,0],"align":"left","angle_deg":0}}}},
            {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,1]}}
        ]}}}]),
    );
    assert!(
        text.contains("Text height -5 mm must be between 0.000001 and 1000000 mm."),
        "{text}"
    );
}

#[test]
fn unknown_tool_names_the_nearest_tool_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let text = refused_text(&mut server, json!([{"tool":"put_prat","id":"box"}]));
    assert_eq!(text.matches("Nearest:").count(), 1, "{text}");
    assert!(text.contains("Nearest: put_part."), "{text}");
}

#[test]
fn placement_failure_refuses_the_batch_and_names_the_operation() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let created = server.call(
        "batch",
        json!({"operations":[{"tool":"put_part","id":"box","part":{"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}}}]}),
    );
    assert_ne!(created["isError"], true, "{created}");
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"move_part","id":"box","translate":[1,0,0]},
            {"tool":"align_parts","id":"box","target":"missing","axes":["x"],"mode":"min"}
        ]),
    );
    assert!(text.starts_with("Batch operation 2: "), "{text}");
    assert!(text.contains("missing"), "{text}");
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}
