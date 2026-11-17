use std::process::Command;

#[test]
fn scan_refuses_a_missing_package() {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["scan", "tests/absent-package"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
}

#[test]
fn help_names_faber() {
    let output = Command::new(env!("CARGO_BIN_EXE_faber")).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("faber scan"));
}
