use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn import_with_reply(root: &Path, reply: serde_json::Value) -> Output {
    let spool = root.join("Faber/Archive");
    fs::create_dir_all(&spool).unwrap();
    fs::write(spool.join("active"), []).unwrap();
    let source = root.join("sample.f3d");
    fs::write(&source, []).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["import"])
        .arg(&source)
        .arg(root.join("project"))
        .env("LOCALAPPDATA", root)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let request = loop {
        if let Some(path) = fs::read_dir(&spool)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("request"))
            .find(|path| path.exists())
        {
            break path;
        }
        if child.try_wait().unwrap().is_some() || start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            panic!("The importer did not send a request");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let requested: std::path::PathBuf =
        serde_json::from_slice(&fs::read(&request).unwrap()).unwrap();
    assert_eq!(requested, source.canonicalize().unwrap());
    let pending = request.with_file_name("pending");
    fs::write(&pending, serde_json::to_vec(&reply).unwrap()).unwrap();
    fs::rename(pending, request.with_file_name("reply")).unwrap();
    child.wait_with_output().unwrap()
}

fn rectangle_reply(volume: f64) -> serde_json::Value {
    json!({"imported":{
        "project":{"schema":1,"revision":0,"name":"Plate","params":{"height":"3"},
            "parts":{"plate":{"name":"Plate","colour":[128,128,128],"geometry":{
                "op":"history","output":"pad","features":[
                    {"id":"profile","geometry":{"op":"sketch","profile":{
                        "outer":{"kind":"rectangle","origin":[0,0],"size":[10,20]}}}},
                    {"id":"pad","geometry":{"op":"extrude_sketch","source":"profile","vector":[0,0,"height"]}}
                ]}}}},
        "volume":volume,"bounds":[[0,0,0],[10,20,3]]
    }})
}

#[test]
fn rebuilds_the_reply_before_it_publishes_an_editable_project() {
    let root = tempfile::tempdir().unwrap();
    let result = import_with_reply(root.path(), rectangle_reply(600.0));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = root.path().join("project");
    let mut project = faber_wiring::open(&path, false, None).unwrap();
    assert!((project.bodies[0].volume - 600.0).abs() < 1e-6);
    project
        .set_params([("height".into(), "5".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 1000.0).abs() < 1e-6);
    drop(project);
    let mut reopened = faber_wiring::open(&path, false, None).unwrap();
    reopened.undo().unwrap();
    assert!((reopened.bodies[0].volume - 600.0).abs() < 1e-6);
}

#[test]
fn a_different_source_volume_leaves_no_project() {
    let root = tempfile::tempdir().unwrap();
    let result = import_with_reply(root.path(), rectangle_reply(800.0));
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("rebuilt solid differs"));
    assert!(!root.path().join("project").exists());
}

#[test]
fn reports_the_first_unsupported_timeline_item() {
    let root = tempfile::tempdir().unwrap();
    let error = "Timeline item 7 (Loft1): Unsupported feature adsk::fusion::LoftFeature";
    let result = import_with_reply(root.path(), json!({"error": error}));
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains(error));
    assert!(!root.path().join("project").exists());
}
