use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_tools_set_inspect_and_render_a_saved_section() {
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
    let call = |request: &mut dyn FnMut(&str, Value) -> Value, name: &str, arguments: Value| {
        request("tools/call", json!({"name":name,"arguments":arguments}))
    };
    request(
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-test","version":"1"}}),
    );
    let open = call(
        &mut request,
        "open",
        json!({"path":dir.path(),"create":true,"name":"Sections"}),
    );
    assert_ne!(open["isError"], true, "{open}");
    let put = call(
        &mut request,
        "put_part",
        json!({"id":"block","part":{"name":"Block","colour":[100,140,170],
            "geometry":{"op":"box","size":[10,20,30]}}}),
    );
    assert_ne!(put["isError"], true, "{put}");
    let set = call(
        &mut request,
        "set_section",
        json!({"id":"mid","section":{"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}}),
    );
    assert_ne!(set["isError"], true, "{set}");
    let info = call(&mut request, "section_info", json!({"id":"mid"}));
    assert_ne!(info["isError"], true, "{info}");
    let text = info["content"][0]["text"].as_str().unwrap();
    let info: Value = serde_json::from_str(text).unwrap();
    let area = info["parts"][0]["area_mm2"].as_f64().unwrap();
    assert!((area - 200.).abs() < 1e-6, "{info}");
    let path = dir.path().join("section.png");
    let render = call(
        &mut request,
        "render",
        json!({"path":path,"section":"mid","width":320,"height":240}),
    );
    assert_ne!(render["isError"], true, "{render}");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!((width, height), (320, 240));
    drop(input);
    child.wait().unwrap();
}
