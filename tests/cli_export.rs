use std::process::Command;

#[test]
fn cli_exports_step_and_keeps_stl_as_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let data = serde_json::json!({
        "schema":1,"revision":0,"name":"Export test","params":{},
        "parts":{"base":{"name":"Base","colour":[80,100,120],
            "geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":30}}}
    });
    std::fs::write(dir.path().join("faber.json"), data.to_string()).unwrap();
    for format in ["step", "stl"] {
        let destination = dir.path().join(format);
        let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
        command.arg("export").arg(dir.path()).arg(&destination);
        if format == "step" {
            command.args(["--format", "step"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let file = destination.join(format!("base.{format}"));
        assert!(std::fs::metadata(&file).unwrap().len() > 100);
        if format == "step" {
            assert!(
                std::fs::read_to_string(file)
                    .unwrap()
                    .contains("ISO-10303-21")
            );
        }
    }
    let destination = dir.path().join("invalid");
    for flags in [
        vec!["--format", "unknown"],
        vec!["--format"],
        vec!["--unknown"],
        vec!["--force", "--format", "unknown"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_faber"))
            .arg("export")
            .arg(dir.path())
            .arg(&destination)
            .args(flags)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!destination.exists());
    }
}

#[test]
fn stl_tolerance_changes_the_triangle_count() {
    let dir = tempfile::tempdir().unwrap();
    let data = serde_json::json!({
        "schema":1,"revision":0,"name":"Tolerance test","params":{},
        "parts":{"ball":{"name":"Ball","colour":[80,100,120],
            "geometry":{"op":"sphere","radius":20}}}
    });
    std::fs::write(dir.path().join("faber.json"), data.to_string()).unwrap();
    let mut triangles = Vec::new();
    for tolerance in ["0.1", "0.01"] {
        let destination = dir.path().join(tolerance);
        let output = Command::new(env!("CARGO_BIN_EXE_faber"))
            .arg("export")
            .arg(dir.path())
            .arg(&destination)
            .args(["--tolerance", tolerance])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(reply["tolerance_mm"], tolerance.parse::<f64>().unwrap());
        triangles.push(reply["files"][0]["triangles"].as_u64().unwrap());
    }
    assert!(triangles[0] < triangles[1], "{triangles:?}");

    let destination = dir.path().join("coarse");
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("export")
        .arg(dir.path())
        .arg(&destination)
        .args(["--tolerance", "2.0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Tolerance 2 mm is outside 0.001 to 1.0 mm."),
        "{stderr}"
    );
    assert!(!destination.exists());
}
