use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn stdio_open_refuses_relative_paths_and_returns_the_absolute_path() {
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

    for path in ["relative/part", "\\drive-less\\part"] {
        let refused = request(
            "tools/call",
            json!({"name":"open","arguments":{"path":path,"create":true}}),
        );
        assert_eq!(refused["isError"], true, "{refused}");
        let text = refused["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            text, "Use an absolute path, for example C:\\work\\part.",
            "{refused}"
        );
    }

    let opened = request(
        "tools/call",
        json!({"name":"open","arguments":{"path":dir.path(),"create":true,"name":"Paths"}}),
    );
    assert_ne!(opened["isError"], true, "{opened}");
    let reply: Value =
        serde_json::from_str(opened["content"][0]["text"].as_str().unwrap()).unwrap();
    let returned = reply["path"].as_str().unwrap();
    assert!(Path::new(returned).is_absolute(), "{returned}");
    assert_eq!(
        std::fs::canonicalize(returned).unwrap(),
        std::fs::canonicalize(dir.path()).unwrap()
    );
    drop(input);
    child.wait().unwrap();
}
