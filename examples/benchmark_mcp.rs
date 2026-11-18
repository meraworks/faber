//! Measure the bin workflow through a real stdio process.
mod common;
use common::Client;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

fn start(executable: &Path, root: &Path, index: usize) -> Client {
    let mut client = Client::start(executable.into(), root.join(format!("calls-{index}.jsonl")));
    client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"faber-benchmark","version":"1"}}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    client
}

fn timed(client: &mut Client, tool: &str, arguments: Value) -> (f64, Value) {
    let start = Instant::now();
    let result = client.tool(tool, arguments);
    (start.elapsed().as_secs_f64() * 1000., result)
}

fn summary(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.
    } else {
        sorted[middle]
    };
    json!({"count":samples.len(),"median_ms":median,
        "min_ms":sorted[0],"max_ms":sorted[sorted.len()-1],"samples_ms":samples})
}

fn main() {
    let destination = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "docs/benchmarks/hex-bin-debug.json".into());
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(if cfg!(windows) { "faber.exe" } else { "faber" });
    let fixture: Value = serde_json::from_str(include_str!("hex-bin/faber.json")).unwrap();
    let mut part: Value = serde_json::from_str(include_str!("hex-bin/parts/bin.json")).unwrap();
    part.as_object_mut().unwrap().remove("schema");
    let scratch = std::env::current_dir().unwrap().join(".cargo");
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = std::fs::canonicalize(scratch).unwrap();
    let directory = tempfile::tempdir_in(&scratch).unwrap();
    assert!(directory.path().starts_with(&scratch));
    let root = directory.path();
    let mut client = start(&executable, root, 0);
    client.tool(
        "open",
        json!({"path":root,"create":true,"name":"Benchmark bin"}),
    );
    let mut operations = Vec::new();
    for (name, expression) in fixture["params"].as_object().unwrap() {
        operations.push(json!({"tool":"set_param","name":name,"expression":expression}));
    }
    operations.push(json!({"tool":"put_part","id":"bin","part":part}));
    let (initial_ms, report) = timed(&mut client, "batch", json!({"operations":operations}));
    assert_eq!(report["part_count"], 1);
    let original_volume = report["parts"][0]["volume_mm3"].as_f64().unwrap();
    assert!(original_volume > 950_000. && original_volume < 960_000.);
    let mut status = Vec::new();
    for _ in 0..20 {
        status.push(timed(&mut client, "status", json!({})).0);
    }
    let mut edits = Vec::new();
    for index in 0..10 {
        let height = if index % 2 == 0 { "350" } else { "340" };
        edits.push(
            timed(
                &mut client,
                "set_param",
                json!({"name":"height","expression":height}),
            )
            .0,
        );
        let report = client.tool("status", json!({}));
        let volume = report["parts"][0]["volume_mm3"].as_f64().unwrap();
        if index % 2 == 0 {
            assert!(volume > original_volume);
        } else {
            assert!((volume - original_volume).abs() < 1e-5);
        }
    }
    let mut images = Vec::new();
    for _ in 0..5 {
        images.push(
            timed(
                &mut client,
                "render",
                json!({"path":root.join("section.png"),
            "width":640,"height":480,"view":"front","part":"bin",
            "section":{"normal":[0,1,0],"offset":0,"reverse":true}}),
            )
            .0,
        );
    }
    drop(client);
    let mut reopen = Vec::new();
    for index in 1..=5 {
        let mut client = start(&executable, root, index);
        let (elapsed, report) = timed(&mut client, "open", json!({"path":root}));
        assert!(
            (report["parts"][0]["volume_mm3"].as_f64().unwrap() - original_volume).abs() < 1e-5
        );
        reopen.push(elapsed);
    }
    let result = json!({"schema":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "debug_assertions":cfg!(debug_assertions),"fixture":fixture,
        "scope":"Stdio round trip including JSON, transcript writes and timing output. Render includes GPU setup when required, PNG file and base64 reply. Process startup and protocol initialisation are excluded from reopen. OS caches are not cleared. No window is opened.",
        "initial_batch_ms":initial_ms,"status":summary(&status),"parameter_edit":summary(&edits),
        "section_render_640x480":summary(&images),"new_process_open":summary(&reopen)});
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&destination, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!("Report: {}", destination.display());
}
