use faber_ports::{AssetStore, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct FileAssets {
    root: PathBuf,
}

impl FileAssets {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
}

fn copy_and_hash(input: &mut File, output: &mut impl Write) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

impl AssetStore for FileAssets {
    fn put(&self, source: &Path) -> Result<String> {
        let mut input = File::open(source).map_err(|e| e.to_string())?;
        let metadata = input.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err("Asset must be a non-empty file".into());
        }
        let directory = self.root.join("assets");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        let key = copy_and_hash(&mut input, temporary.as_file_mut())?;
        if temporary
            .as_file()
            .metadata()
            .map_err(|e| e.to_string())?
            .len()
            == 0
        {
            return Err("Asset must be a non-empty file".into());
        }
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        match temporary.persist_noclobber(directory.join(&key)) {
            Ok(_) => (),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.resolve(&key)?;
            }
            Err(error) => return Err(error.to_string()),
        }
        Ok(key)
    }

    fn resolve(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64
            || !key
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("Invalid asset key".into());
        }
        let path = self.root.join("assets").join(key);
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Asset must be a regular file".into());
        }
        let resolved = path.canonicalize().map_err(|e| e.to_string())?;
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        if !resolved.starts_with(root) {
            return Err("Asset is outside the project".into());
        }
        let mut input = File::open(&resolved).map_err(|e| e.to_string())?;
        if copy_and_hash(&mut input, &mut std::io::sink())? != key {
            return Err("Asset content does not match its hash".into());
        }
        Ok(resolved)
    }
}
