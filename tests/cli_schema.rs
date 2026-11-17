use serde_json::Value;
use std::process::Command;

#[test]
fn schema_is_available_without_a_project_and_contains_editable_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .arg("schema")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    for field in ["schema", "revision", "name", "params", "parts"] {
        assert!(schema["properties"].get(field).is_some(), "{field}");
    }
    let text = output.stdout;
    let text = String::from_utf8(text).unwrap();
    for operation in [
        "history",
        "loft_sketches",
        "revolve_sketch",
        "shell",
        "fillet",
        "circular_pattern",
    ] {
        assert!(text.contains(&format!("\"{operation}\"")), "{operation}");
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    let invalid = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["schema", "unknown"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
}
