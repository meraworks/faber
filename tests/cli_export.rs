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
