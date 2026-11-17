use faber_clash::package;
use std::path::Path;

fn write_package(dir: &Path, part: &str, bin: &[u8]) {
    let manifest = format!(
        r#"{{"parts":[{{"name":"a","kind":"printed","movable":false,{part}}}],"proximity_mm":0.1}}"#
    );
    std::fs::write(dir.join("scene.json"), manifest).unwrap();
    std::fs::write(dir.join("scene.bin"), bin).unwrap();
}

fn refused(part: &str, bin: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    write_package(dir.path(), part, bin);
    match package::load(dir.path()) {
        Ok(_) => panic!("malformed package was accepted"),
        Err(error) => error,
    }
}

#[test]
fn huge_vertex_count_is_refused_without_panic() {
    let error = refused(
        r#""vertex_count":18446744073709551615,"triangle_count":1,"offset":0"#,
        &[0u8; 64],
    );
    assert!(
        error.contains("too short") || error.contains("overflow"),
        "{error}"
    );
}

#[test]
fn huge_triangle_count_is_refused_without_panic() {
    let error = refused(
        r#""vertex_count":0,"triangle_count":1537228672809129301,"offset":0"#,
        &[0u8; 64],
    );
    assert!(
        error.contains("too short") || error.contains("overflow"),
        "{error}"
    );
}

#[test]
fn offset_past_end_is_refused() {
    let error = refused(
        r#""vertex_count":0,"triangle_count":0,"offset":1000"#,
        &[0u8; 24],
    );
    assert!(error.contains("too short"), "{error}");
}

#[test]
fn offset_near_usize_max_is_refused_without_wrap() {
    let error = refused(
        r#""vertex_count":1,"triangle_count":1,"offset":18446744073709551600"#,
        &[0u8; 64],
    );
    assert!(
        error.contains("too short") || error.contains("overflow"),
        "{error}"
    );
}

#[test]
fn triangle_index_out_of_range_is_refused() {
    let mut bin = Vec::new();
    for v in [0f64, 0., 0., 1., 0., 0., 0., 1., 0.] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u32, 1, 7] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let error = refused(r#""vertex_count":3,"triangle_count":1,"offset":0"#, &bin);
    assert!(error.contains("out of range"), "{error}");
}
