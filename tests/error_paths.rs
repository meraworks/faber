//! Input and build errors name the JSON path, the feature and the operation.

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
        server.send(
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

    /// Sends one request and returns the whole reply object.
    fn send(&mut self, method: &str, params: Value) -> Value {
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
        serde_json::from_str(&line).unwrap()
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.send("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    /// Returns the error text of a call that the server refuses.
    fn refusal(&mut self, name: &str, arguments: Value) -> String {
        let reply = self.send("tools/call", json!({"name":name,"arguments":arguments}));
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn open_project(dir: &std::path::Path) -> Server {
    let mut server = Server::start();
    let open = server.call(
        "open",
        json!({"path":dir,"create":true,"name":"Error paths"}),
    );
    assert_ne!(open["isError"], true, "{open}");
    server
}

fn block_part() -> Value {
    json!({"name":"Block","colour":[100,140,170],"geometry":{"op":"history","output":"block","features":[
        {"id":"block","geometry":{"op":"box","size":[10,20,30]}}
    ]}})
}

fn refused_text(server: &mut Server, operations: Value) -> String {
    let reply = server.call("batch", json!({"operations": operations}));
    assert_eq!(reply["isError"], true, "{reply}");
    reply["content"][0]["text"].as_str().unwrap().to_string()
}

#[test]
fn batch_list_for_shell_faces_names_the_operation_and_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let before = std::fs::read(dir.path().join("faber.json")).unwrap();
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"put_part","id":"base","part":block_part()},
            {"tool":"add_feature","part":"base","feature":{"id":"walls","geometry":{
                "op":"shell","source":"block","faces":[[0,0,1]],"thickness":1}}}
        ]),
    );
    assert!(text.contains("Batch operation 2"), "{text}");
    assert!(
        text.contains("operations[1].feature.geometry.faces"),
        "{text}"
    );
    assert_eq!(
        std::fs::read(dir.path().join("faber.json")).unwrap(),
        before
    );
}

#[test]
fn put_part_wrong_chamfer_edges_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let mut part = block_part();
    part["geometry"]["features"].as_array_mut().unwrap().push(
        json!({"id":"round","geometry":{"op":"chamfer","source":"block",
            "edges":[0,0,1],"distance":1}}),
    );
    part["geometry"]["output"] = json!("round");
    let message = server.refusal("put_part", json!({"id":"base","part":part}));
    assert!(
        message.contains("part.geometry.features[1].geometry.edges"),
        "{message}"
    );
}

#[test]
fn direct_add_feature_wrong_sketch_size_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    server.call("put_part", json!({"id":"base","part":block_part()}));
    let message = server.refusal(
        "add_feature",
        json!({"part":"base","feature":{"id":"plate","geometry":{"op":"sketch",
            "profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[20,{"value":1}]}}}}}),
    );
    assert!(message.contains("profile.outer.size"), "{message}");
}

#[test]
fn kernel_failure_names_the_part_feature_and_operation() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = open_project(dir.path());
    let text = refused_text(
        &mut server,
        json!([
            {"tool":"put_part","id":"rounded_block","part":{"name":"Rounded block","colour":[100,140,170],
                "geometry":{"op":"history","output":"corner_round","features":[
                    {"id":"block","geometry":{"op":"box","size":[10,20,30]}},
                    {"id":"corner_round","geometry":{"op":"fillet","source":"block",
                        "edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":100}}
                ]}}}
        ]),
    );
    assert!(text.contains("rounded_block"), "{text}");
    assert!(text.contains("corner_round"), "{text}");
    assert!(text.contains("(fillet)"), "{text}");
    assert!(text.contains("tested value"), "{text}");
}
