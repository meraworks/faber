use faber_core::Project;
use faber_model::{Part, Scalar};
use serde_json::{Value, json};

pub fn fixture() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Face tools".into())).unwrap();
    (dir, p)
}
pub fn feature(id: &str, geometry: Value) -> Value {
    json!({"id":id,"geometry":geometry})
}
pub fn blank() -> Value {
    feature("base", json!({"op":"box","size":[20,20,20]}))
}
pub fn top() -> Value {
    json!({"normal":[0,0,1],"tolerance":0.001,"expect":1})
}
pub fn history(features: &[Value]) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Plate","colour":[80,120,150],"geometry":{"op":"history","features":features,"output":features.last().unwrap()["id"]}})).unwrap()
}
pub fn build(features: &[Value]) -> (Project, tempfile::TempDir) {
    let (dir, mut p) = fixture();
    p.put_part("plate".into(), history(features)).unwrap();
    (p, dir)
}
pub fn refused(features: &[Value], message: &str) {
    let (mut p, dir) = build(&[blank()]);
    fn saved(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut files = std::collections::BTreeMap::new();
        for item in std::fs::read_dir(root).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                if !path.file_name().unwrap().to_string_lossy().starts_with('.') {
                    files.extend(saved(&path));
                }
            } else if path.extension().is_some_and(|s| s == "json") {
                files.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        files
    }
    let before = saved(dir.path());
    assert!(!before.is_empty());
    let err = p.put_part("plate".into(), history(features)).unwrap_err();
    assert!(err.contains(message), "{err}");
    assert_eq!(before, saved(dir.path()));
    close(p.bodies[0].volume, 8000.);
}
pub fn close(actual: f64, want: f64) {
    assert!((actual - want).abs() < 1e-6, "{actual} vs {want}")
}
pub fn rounded(source: &str) -> Value {
    feature(
        "round",
        json!({"op":"fillet","source":source,"edges":{"near_centre":[0,10,20],"tolerance":0.001,"expect":1},"radius":2}),
    )
}
pub fn drilled() -> Value {
    feature(
        "hole",
        json!({"op":"hole","source":"base","diameter":5,"depth":20,"placement":{"translation":[10,10,0],"rotation":[0,0,0]}}),
    )
}
pub fn cylinder(radius: f64) -> Value {
    json!({"cylinder_radius":radius,"expect":1,"tolerance":0.001})
}
