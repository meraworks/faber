//! Build the bin through stdio MCP.
mod common;
use common::Client;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/hex-bin"));
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
    client.request("initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-example","version":"0.1.0"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let tools = client.request("tools/list", json!({}));
    assert!(
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "put_part")
    );
    client.tool("guide", json!({}));
    client.tool(
        "open",
        json!({"path":root,"create":true,"name":"Hexagonal bin"}),
    );
    let radius = 150f64;
    let wall = 2.4;
    let height = 340.;
    let floor = 4.;
    let inner = radius - wall / (std::f64::consts::PI / 6.).cos();
    for (name, expression) in [
        ("radius", "150"),
        ("wall", "2.4"),
        ("height", "340"),
        ("floor", "4"),
        ("inner", "radius - wall / cos(30)"),
    ] {
        client.tool("set_param", json!({"name":name,"expression":expression}));
    }
    let report=client.tool("put_part",json!({"id":"bin","part":{
        "name":"Hexagonal bin","group":"Body","colour":[151,172,176],
        "note":"Open top. Wall: 2.4 mm. Base: 4 mm.",
        "geometry":{"op":"history","output":"hollow","features":[
            {"id":"outer_sketch","geometry":{"op":"sketch","profile":{"outer":{"kind":"regular_polygon","centre":[0,0],"radius":"radius","sides":6,"rotation":0}}}},
            {"id":"outer","geometry":{"op":"extrude_sketch","source":"outer_sketch","vector":[0,0,"height"]}},
            {"id":"cavity_sketch","geometry":{"op":"sketch","profile":{"outer":{"kind":"regular_polygon","centre":[0,0],"radius":"inner","sides":6,"rotation":0}}}},
            {"id":"cavity","geometry":{"op":"extrude_sketch","source":"cavity_sketch","vector":[0,0,"height - floor"]}},
            {"id":"raised_cavity","geometry":{"op":"transform","source":"cavity","placement":{"translation":[0,0,"floor"]}}},
            {"id":"hollow","geometry":{"op":"difference","left":"outer","right":"raised_cavity"}}
        ]}
    }}));
    assert_eq!(report["part_count"], 1);
    let expected =
        3. * 3f64.sqrt() / 2. * (radius * radius * height - inner * inner * (height - floor));
    let measured = report["parts"][0]["volume_mm3"].as_f64().unwrap();
    assert!((expected - measured).abs() < 1e-5);
    client.tool("status", json!({}));
    client.tool("export", json!({"directory":root.join("out")}));
    client.tool(
        "export",
        json!({"directory":root.join("out"),"format":"step"}),
    );
    client.tool(
        "render",
        json!({"path":root.join("out/exterior.png"),"width":1400,"height":1200}),
    );
    client.tool("render",json!({"path":root.join("out/section.png"),"section":{"normal":[0,-1,0],"offset":0,"reverse":false},"width":1400,"height":1200}));
    println!("Saved {}", root.display());
    println!(
        "Material: {:.2} cm3; capacity: {:.2} litres",
        measured / 1000.,
        3. * 3f64.sqrt() / 2. * inner * inner * (height - floor) / 1e6
    );
}
