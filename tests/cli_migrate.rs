use std::process::Command;

fn migrate(dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("migrate")
        .arg(dir)
        .output()
        .unwrap()
}

#[test]
fn cli_migrate_splits_an_old_project_and_prints_only_changes() {
    let dir = tempfile::tempdir().unwrap();
    let data = serde_json::json!({
        "schema":1,"revision":3,"name":"Migrate test","params":{},
        "parts":{"base":{"name":"Base","colour":[80,100,120],
            "geometry":{"op":"box","size":[10,20,30]}}}
    });
    std::fs::write(dir.path().join("faber.json"), data.to_string()).unwrap();
    let first = migrate(dir.path());
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let printed = String::from_utf8_lossy(&first.stdout);
    assert!(printed.contains("base.json"), "{printed}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("faber.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema"], 2);
    assert!(manifest.get("parts").is_none());
    let second = migrate(dir.path());
    assert!(second.status.success());
    assert!(
        second.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&second.stdout)
    );
}
