use faber_files::layout;
use faber_model::{Placement, ProjectData, Scalar};
use std::fs;
use std::path::{Path, PathBuf};

const ONE_FILE: &str = r#"{
  "schema": 1,
  "revision": 4,
  "name": "Split",
  "params": {"width": "20"},
  "parts": {
    "base": {"name": "Base", "geometry": {"op": "box", "size": [20, 20, 4]}, "placement": {"translation": [0, 0, 0], "rotation": [0, 0, 0]}, "colour": [10, 20, 30], "group": "frame", "note": "n"},
    "lid": {"name": "Lid", "geometry": {"op": "box", "size": [20, 20, 2]}, "placement": {"translation": [0, 0, 10], "rotation": [0, 0, 0]}, "colour": [40, 50, 60]},
    "pin": {"name": "Pin", "geometry": {"op": "box", "size": [2, 2, 8]}, "placement": {"translation": [5, 5, 4], "rotation": [0, 0, 0]}, "colour": [70, 80, 90]}
  },
  "contacts": {"base|pin": {"a": "base", "b": "pin", "note": "", "max_depth_mm": 0.0}},
  "motion": {"joints": {"hinge": {"kind": "revolute", "parts": ["base", "lid"], "origin": [0, 0, 4], "axis": [1, 0, 0]}}},
  "sections": {"mid": {"plane": {"kind": "plane", "origin": [0, 0, 2], "normal": [0, 0, 1]}}}
}"#;

const LAYOUT_FILES: [&str; 7] = [
    "faber.json",
    "parts/base.json",
    "parts/lid.json",
    "parts/pin.json",
    "placement.json",
    "assembly.json",
    "analysis.json",
];

fn fixture() -> ProjectData {
    serde_json::from_str(ONE_FILE).unwrap()
}

fn relative(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    names.sort();
    names
}

fn manifest(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join("faber.json")).unwrap()).unwrap()
}

#[test]
fn split_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let data = fixture();
    layout::write(dir.path(), &data).unwrap();
    for file in LAYOUT_FILES {
        assert!(dir.path().join(file).is_file(), "{file} is missing");
    }
    assert_eq!(layout::read(dir.path()).unwrap(), data);
}

#[test]
fn schema_1_opens_and_migrates_on_save() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), ONE_FILE).unwrap();
    let data = layout::read(dir.path()).unwrap();
    assert_eq!(data.parts.len(), 3);
    layout::write(dir.path(), &data).unwrap();
    for file in LAYOUT_FILES {
        assert!(dir.path().join(file).is_file(), "{file} is missing");
    }
    let manifest = manifest(dir.path());
    assert_eq!(manifest["schema"], 2);
    assert!(manifest.get("parts").is_none());
    assert_eq!(layout::read(dir.path()).unwrap(), data);
}

#[test]
fn move_writes_only_placement_and_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    layout::write(dir.path(), &data).unwrap();
    data.revision += 1;
    data.parts.get_mut("lid").unwrap().placement.translation[2] = Scalar::Number(12.0);
    let changed = layout::write(dir.path(), &data).unwrap();
    assert_eq!(
        relative(dir.path(), &changed),
        ["faber.json", "placement.json"]
    );
    assert_eq!(layout::read(dir.path()).unwrap(), data);
}

#[test]
fn removed_part_file_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    layout::write(dir.path(), &data).unwrap();
    data.parts.remove("pin");
    data.revision += 1;
    layout::write(dir.path(), &data).unwrap();
    assert!(!dir.path().join("parts/pin.json").exists());
    let placement: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("placement.json")).unwrap()).unwrap();
    assert!(placement["parts"].get("pin").is_none());
    assert_eq!(layout::read(dir.path()).unwrap(), data);
}

#[test]
fn empty_assembly_and_analysis_files_are_absent() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    layout::write(dir.path(), &data).unwrap();
    assert!(dir.path().join("assembly.json").is_file());
    assert!(dir.path().join("analysis.json").is_file());
    data.contacts.clear();
    data.motion = Default::default();
    data.sections.clear();
    data.revision += 1;
    layout::write(dir.path(), &data).unwrap();
    assert!(!dir.path().join("assembly.json").exists());
    assert!(!dir.path().join("analysis.json").exists());
    assert_eq!(layout::read(dir.path()).unwrap(), data);
}

#[test]
fn bad_part_file_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    fs::write(
        dir.path().join("parts/lid.json"),
        "{\n  \"schema\": 1,\n  \"name\": \"Lid\",\n  \"colour\": [40, 50,,]\n}",
    )
    .unwrap();
    let error = layout::read(dir.path()).unwrap_err();
    assert!(error.contains("parts/lid.json"), "{error}");
    assert!(error.contains("line 4 column"), "{error}");
}

#[test]
fn part_file_without_placement_gets_identity() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    fs::remove_file(dir.path().join("placement.json")).unwrap();
    let data = layout::read(dir.path()).unwrap();
    for part in data.parts.values() {
        assert_eq!(part.placement, Placement::default());
    }
}

#[test]
fn placement_of_unknown_part_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    let path = dir.path().join("placement.json");
    let mut placement: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    placement["parts"]["ghost"] =
        serde_json::json!({"translation": [0, 0, 0], "rotation": [0, 0, 0]});
    fs::write(&path, serde_json::to_vec(&placement).unwrap()).unwrap();
    let error = layout::read(dir.path()).unwrap_err();
    assert!(error.contains("ghost"), "{error}");
}

#[test]
fn part_ids_are_safe_file_names() {
    for id in ["a/b", "..", "", "con", "x y"] {
        let dir = tempfile::tempdir().unwrap();
        let mut data = fixture();
        let part = data.parts["lid"].clone();
        data.parts.insert(id.to_string(), part);
        assert!(
            layout::write(dir.path(), &data).is_err(),
            "id {id:?} was written"
        );
    }
}

fn edit_json(path: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    change(&mut value);
    // Keep the final newline of the layout writer, so that a length change is only the edit.
    let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

#[test]
fn fingerprint_changes_after_a_part_edit() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    let manifest = fs::read(dir.path().join("faber.json")).unwrap();
    let before = layout::fingerprint(dir.path()).unwrap();
    // Only the part file changes. The manifest stays the same.
    edit_json(&dir.path().join("parts/lid.json"), |part| {
        part["name"] = serde_json::json!("Lid, changed");
    });
    let after_part = layout::fingerprint(dir.path()).unwrap();
    assert_ne!(after_part, before);
    // Only the placement file changes. The manifest stays the same.
    edit_json(&dir.path().join("placement.json"), |file| {
        file["parts"]["lid"]["translation"][2] = serde_json::json!(120.0);
    });
    assert_ne!(layout::fingerprint(dir.path()).unwrap(), after_part);
    assert_eq!(fs::read(dir.path().join("faber.json")).unwrap(), manifest);
}

#[test]
fn interrupted_removal_still_reads() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    layout::write(dir.path(), &data).unwrap();
    let placement_path = dir.path().join("placement.json");
    let old_placement = fs::read(&placement_path).unwrap();
    data.parts.remove("pin");
    data.revision += 1;
    // Stop the write at the placement step. A folder cannot be replaced by a file on
    // every system, so this forces the write to fail there.
    fs::remove_file(&placement_path).unwrap();
    fs::create_dir(&placement_path).unwrap();
    assert!(layout::write(dir.path(), &data).is_err());
    fs::remove_dir(&placement_path).unwrap();
    fs::write(&placement_path, old_placement).unwrap();
    let read_back = layout::read(dir.path()).unwrap();
    assert_eq!(read_back.parts.len(), 3);
    assert!(read_back.parts.contains_key("pin"));
}

#[test]
fn placement_without_removed_part_still_reads() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    // The new placement is saved. The old part file is still there.
    edit_json(&dir.path().join("placement.json"), |file| {
        file["parts"].as_object_mut().unwrap().remove("pin");
    });
    let data = layout::read(dir.path()).unwrap();
    assert_eq!(data.parts["pin"].placement, Placement::default());
}

#[test]
fn placement_file_without_entry_gets_identity() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    edit_json(&dir.path().join("placement.json"), |file| {
        file["parts"].as_object_mut().unwrap().remove("lid");
    });
    let data = layout::read(dir.path()).unwrap();
    assert_eq!(data.parts["lid"].placement, Placement::default());
    assert_eq!(
        data.parts["base"].placement.translation[2],
        Scalar::Number(0.)
    );
}

#[test]
fn case_change_of_a_part_name_keeps_the_part() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    let lid = data.parts.remove("lid").unwrap();
    data.parts.insert("Lid".into(), lid);
    layout::write(dir.path(), &data).unwrap();
    let mut renamed = fixture();
    renamed.revision += 1;
    layout::write(dir.path(), &renamed).unwrap();
    let mut names: Vec<String> = fs::read_dir(dir.path().join("parts"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["base.json", "lid.json", "pin.json"]);
    assert_eq!(layout::read(dir.path()).unwrap(), renamed);
}

#[test]
fn newer_manifest_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    layout::write(dir.path(), &fixture()).unwrap();
    let path = dir.path().join("faber.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["schema"] = serde_json::json!(3);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = layout::read(dir.path()).unwrap_err();
    assert!(error.contains("Unsupported project schema 3"), "{error}");
}
