use faber_core::Project;
use faber_files::{FileAssets, FileFeatureCache, FileStore};
use faber_kernel_occt::OcctKernel;
use faber_ports::{FeatureCache, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// One part with three features. Feature a depends on w. Feature b is independent.
const PROJECT: &str = r#"{
  "schema": 1,
  "revision": 0,
  "name": "Cache",
  "params": { "w": "20" },
  "parts": {
    "p": {
      "name": "Part",
      "colour": [10, 20, 30],
      "geometry": {
        "op": "history",
        "output": "sum",
        "features": [
          { "id": "a", "geometry": { "op": "box", "size": ["w", 10, 10] } },
          { "id": "b", "geometry": { "op": "box", "size": [5, 5, 5] } },
          { "id": "sum", "geometry": { "op": "union", "left": "a", "right": "b" } }
        ]
      }
    }
  }
}"#;

/// Counts the writes and reads of the feature cache. Each write follows one feature build.
struct Counting {
    inner: FileFeatureCache,
    puts: AtomicUsize,
    hits: AtomicUsize,
}

impl FeatureCache for Counting {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let bytes = self.inner.get(key);
        if bytes.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        bytes
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.puts.fetch_add(1, Ordering::Relaxed);
        self.inner.put(key, bytes)
    }
}

struct Opened {
    project: Project,
    cache: Arc<Counting>,
}

fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), PROJECT).unwrap();
    dir
}

fn open(root: &Path) -> Opened {
    let store = Arc::new(FileStore::open(root).unwrap());
    let cache = Arc::new(Counting {
        inner: FileFeatureCache::new(root),
        puts: AtomicUsize::new(0),
        hits: AtomicUsize::new(0),
    });
    let project = Project::open_with_cache(
        store,
        Arc::new(OcctKernel),
        Some(Arc::new(FileAssets::new(root))),
        Some(cache.clone()),
    )
    .unwrap();
    Opened { project, cache }
}

fn cache_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(root.join(".faber").join("cache"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no cache entries were written");
    files
}

#[test]
fn second_open_reuses_persistent_features() {
    let dir = project_dir();
    let first = open(dir.path());
    assert_eq!(first.cache.puts.load(Ordering::Relaxed), 3);
    let volume = first.project.bodies[0].volume;
    let bounds = first.project.bodies[0].bounds;
    drop(first);

    let second = open(dir.path());
    assert_eq!(
        second.cache.puts.load(Ordering::Relaxed),
        0,
        "features were rebuilt"
    );
    assert_eq!(second.cache.hits.load(Ordering::Relaxed), 3);
    assert_eq!(second.project.bodies[0].volume, volume);
    assert_eq!(second.project.bodies[0].bounds, bounds);
}

#[test]
fn changed_parameter_rebuilds_only_dependants() {
    let dir = project_dir();
    let mut first = open(dir.path());
    let puts = first.cache.puts.load(Ordering::Relaxed);
    assert_eq!(puts, 3);
    first
        .project
        .set_params(BTreeMap::from([("w".to_string(), "30".to_string())]))
        .unwrap();
    // The change to w builds a and sum. Feature b is unchanged.
    assert_eq!(first.cache.puts.load(Ordering::Relaxed) - puts, 2);
    let expected = 30. * 10. * 10.;
    assert!((first.project.bodies[0].volume - expected).abs() < 1e-6);
    drop(first);

    let second = open(dir.path());
    assert_eq!(second.cache.puts.load(Ordering::Relaxed), 0);
    assert!((second.project.bodies[0].volume - expected).abs() < 1e-6);
}

#[test]
fn corrupted_entries_rebuild_with_the_same_result() {
    let dir = project_dir();
    let first = open(dir.path());
    let volume = first.project.bodies[0].volume;
    drop(first);

    for file in cache_files(dir.path()) {
        let mut bytes = fs::read(&file).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        fs::write(&file, bytes).unwrap();
    }

    let second = open(dir.path());
    assert_eq!(
        second.cache.puts.load(Ordering::Relaxed),
        3,
        "corrupt entries were used"
    );
    assert_eq!(second.project.bodies[0].volume, volume);
}

#[test]
fn entries_from_another_format_version_are_ignored() {
    let dir = project_dir();
    let first = open(dir.path());
    let volume = first.project.bodies[0].volume;
    drop(first);

    for file in cache_files(dir.path()) {
        let mut bytes = fs::read(&file).unwrap();
        // The first four bytes hold the format marker.
        bytes[..4].copy_from_slice(b"FBC0");
        fs::write(&file, bytes).unwrap();
    }

    let second = open(dir.path());
    assert_eq!(
        second.cache.hits.load(Ordering::Relaxed),
        0,
        "old format was read"
    );
    assert_eq!(second.cache.puts.load(Ordering::Relaxed), 3);
    assert_eq!(second.project.bodies[0].volume, volume);
}

#[test]
fn reopened_face_history_matches_a_fresh_build() {
    let dir = project_dir();
    let first = open(dir.path());
    let (_, faces) = first.project.feature_topology("p", "sum").unwrap();
    let expected = format!("{faces:?}");
    assert!(
        expected.contains("created_by: Some"),
        "no face history in the fresh build"
    );
    drop(first);

    let second = open(dir.path());
    assert_eq!(second.cache.hits.load(Ordering::Relaxed), 3);
    let (_, faces) = second.project.feature_topology("p", "sum").unwrap();
    assert_eq!(format!("{faces:?}"), expected);
}

// Boxes a and b have equal geometry, so only the feature id tells them apart.
// Box b is moved by m. Union u keeps the faces of a and m apart.
// The fillet selects the faces that b made.
const TWIN: &str = r#"{
  "schema": 1,
  "revision": 0,
  "name": "Cache",
  "params": {},
  "parts": {
    "p": {
      "name": "Part",
      "colour": [10, 20, 30],
      "geometry": {
        "op": "history",
        "output": "f",
        "features": [
          { "id": "a", "geometry": { "op": "box", "size": [10, 10, 10] } },
          { "id": "b", "geometry": { "op": "box", "size": [10, 10, 10] } },
          { "id": "m", "geometry": { "op": "transform", "source": "b", "placement": { "translation": [20, 0, 0] } } },
          { "id": "u", "geometry": { "op": "union", "left": "a", "right": "m" } },
          { "id": "f", "geometry": { "op": "fillet", "source": "u", "radius": 1,
            "edges": {
              "of_face": { "created_by": "b", "normal": [1, 0, 0], "expect": 1, "tolerance": 0.000001 },
              "expect": 4, "tolerance": 0.000001
            } } }
        ]
      }
    }
  }
}"#;

fn project_dir_with(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("faber.json"), text).unwrap();
    dir
}

fn open_without_cache(root: &Path) -> Project {
    let store = Arc::new(FileStore::open(root).unwrap());
    Project::open_with_cache(
        store,
        Arc::new(OcctKernel),
        Some(Arc::new(FileAssets::new(root))),
        None,
    )
    .unwrap()
}

/// Counts the faces of the output feature that the given feature id created.
fn faces_created_by(project: &Project, label: &str) -> usize {
    let (_, faces) = project.feature_topology("p", "f").unwrap();
    faces
        .iter()
        .filter(|face| {
            face.created_by
                .as_ref()
                .is_some_and(|names| names.iter().any(|name| name == label))
        })
        .count()
}

#[test]
fn equal_features_keep_their_own_face_origins() {
    let fresh_dir = project_dir_with(TWIN);
    let fresh = open_without_cache(fresh_dir.path());
    let volume = fresh.bodies[0].volume;
    let expected = faces_created_by(&fresh, "b");
    assert!(expected > 0, "the fresh build has no faces of b");

    let dir = project_dir_with(TWIN);
    let first = open(dir.path());
    assert!((first.project.bodies[0].volume - volume).abs() < 1e-6);
    assert_eq!(faces_created_by(&first.project, "b"), expected);
    drop(first);

    let second = open(dir.path());
    assert_eq!(second.cache.hits.load(Ordering::Relaxed), 5);
    assert!((second.project.bodies[0].volume - volume).abs() < 1e-6);
    assert_eq!(faces_created_by(&second.project, "b"), expected);
}

#[test]
fn renamed_feature_keeps_its_own_face_origins() {
    let dir = project_dir_with(TWIN);
    drop(open(dir.path()));

    let renamed = TWIN.replace("\"b\"", "\"c\"");
    fs::write(dir.path().join("faber.json"), &renamed).unwrap();
    let fresh_dir = project_dir_with(&renamed);
    let fresh = open_without_cache(fresh_dir.path());
    let volume = fresh.bodies[0].volume;
    let expected = faces_created_by(&fresh, "c");
    assert!(expected > 0, "the fresh build has no faces of c");

    let second = open(dir.path());
    assert!((second.project.bodies[0].volume - volume).abs() < 1e-6);
    assert_eq!(faces_created_by(&second.project, "c"), expected);
}
