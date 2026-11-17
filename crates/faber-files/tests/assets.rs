use faber_files::FileAssets;
use faber_ports::AssetStore;

#[test]
fn assets_preserve_bytes_deduplicate_and_refuse_changed_content() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.model");
    let bytes = b"original model bytes\r\n";
    std::fs::write(&source, bytes).unwrap();
    let root = directory.path().join("project");
    let assets = FileAssets::new(&root);
    let key = assets.put(&source).unwrap();
    assert_eq!(key.len(), 64);
    assert_eq!(assets.put(&source).unwrap(), key);
    let stored = assets.resolve(&key).unwrap();
    assert_eq!(std::fs::read(&stored).unwrap(), bytes);
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    std::fs::remove_file(&source).unwrap();
    assert_eq!(FileAssets::new(&root).resolve(&key).unwrap(), stored);
    std::fs::write(&stored, b"changed").unwrap();
    assert!(assets.resolve(&key).unwrap_err().contains("hash"));
    std::fs::write(&source, bytes).unwrap();
    assert!(assets.put(&source).unwrap_err().contains("hash"));
    assert_eq!(std::fs::read(&stored).unwrap(), b"changed");
    for key in ["../source.model", "", "abc", &"G".repeat(64)] {
        assert!(assets.resolve(key).is_err());
    }
}

#[test]
fn concurrent_imports_publish_one_complete_asset() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let source = directory.path().join("source");
    std::fs::write(&source, b"abc").unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| FileAssets::new(&root).put(&source).unwrap()))
            .collect();
        for handle in handles {
            assert_eq!(
                handle.join().unwrap(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }
    });
    assert_eq!(std::fs::read_dir(root.join("assets")).unwrap().count(), 1);
}

#[test]
fn missing_and_empty_inputs_do_not_create_assets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let assets = FileAssets::new(&root);
    assert!(assets.put(&directory.path().join("missing")).is_err());
    let empty = directory.path().join("empty");
    std::fs::write(&empty, []).unwrap();
    assert!(assets.put(&empty).is_err());
    assert!(!root.exists());
}
