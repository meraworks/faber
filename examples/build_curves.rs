//! Build the curved examples through stdio MCP.
mod common;
#[path = "common/curves.rs"]
mod curves;
use common::Client;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/curved-parts"));
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(if cfg!(windows) { "faber.exe" } else { "faber" });
    let mut client = Client::start(executable, root.join("mcp-transcript.jsonl"));
    client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-curves","version":"1"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client.tool(
        "open",
        json!({"path":root,"create":true,"name":"Curved parts"}),
    );
    client.tool("batch", json!({"operations":curves::operations()}));
    client.tool(
        "export",
        json!({"directory":root.join("out"),"format":"step"}),
    );
    client.tool(
        "export",
        json!({"directory":root.join("out"),"format":"stl"}),
    );
    client.tool(
        "render",
        json!({"path":root.join("out/exterior.png"),"width":1400,"height":1000}),
    );
    client.tool("render", json!({"path":root.join("out/section.png"),"width":1400,"height":1000,"section":{"normal":[0,1,0],"offset":0,"reverse":true}}));
}
