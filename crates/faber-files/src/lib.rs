//! Project storage and mesh file output.

mod assets;
mod feature_cache;
mod history;
pub mod layout;
mod mesh_input;
mod threemf;
pub use threemf::ThreeMf;
mod vector;
pub use assets::FileAssets;
pub use feature_cache::FileFeatureCache;
pub use layout::{fingerprint, read};
pub use vector::VectorWriter;

use faber_model::ProjectData;
use faber_ports::{Exchange, Mesh, ProjectStore, Result};
use glam::DVec3;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct FileStore {
    root: PathBuf,
    _lock: File,
}

impl FileStore {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".faber.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "Another writer has this project open".to_string())?;
        let store = Self {
            root: root.to_path_buf(),
            _lock: lock,
        };
        store.complete_interrupted_save()?;
        Ok(store)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path)
        .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
    Ok(())
}

impl ProjectStore for FileStore {
    fn checkpoint(&self, name: &str, expected: &ProjectData) -> Result<()> {
        let path = self.checkpoint_path(name)?;
        if self.load()? != *expected {
            return Err("Project changed before checkpoint".into());
        }
        let parent = path.parent().ok_or("Checkpoint has no directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(expected).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        if self.load()? != *expected {
            return Err("Project changed before checkpoint".into());
        }
        file.persist_noclobber(path)
            .map_err(|e| format!("Cannot save checkpoint: {e}"))?;
        Ok(())
    }

    fn read_checkpoint(&self, name: &str) -> Result<ProjectData> {
        let bytes = std::fs::read(self.checkpoint_path(name)?)
            .map_err(|e| format!("Cannot read checkpoint: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read checkpoint data: {e}"))
    }
    fn load(&self) -> Result<ProjectData> {
        read(&self.root)
    }
    fn save(&self, expected_revision: Option<u64>, data: &ProjectData) -> Result<()> {
        let path = self.root.join("faber.json");
        match expected_revision {
            Some(revision) if self.load()?.revision != revision => {
                return Err("Project changed before save".into());
            }
            None if path.exists() => return Err("Project already exists".into()),
            _ => {}
        }
        let current = expected_revision.map(|_| self.load()).transpose()?;
        self.commit_history(current.as_ref(), data, None)
    }
    fn save_checked(&self, expected: &ProjectData, data: &ProjectData) -> Result<()> {
        self.commit_history(Some(expected), data, None)
    }
    fn history_target(&self, action: faber_ports::HistoryMove) -> Result<ProjectData> {
        self.target(action)
    }
    fn save_history(
        &self,
        expected: &ProjectData,
        data: &ProjectData,
        action: faber_ports::HistoryMove,
    ) -> Result<()> {
        self.commit_history(Some(expected), data, Some(action))
    }
    fn save_regenerated(&self, expected: &ProjectData, data: &ProjectData) -> Result<()> {
        self.commit_regenerated(expected, data)
    }
}

pub struct MeshExchange;

impl Exchange for MeshExchange {
    fn write(&self, mesh: &Mesh, path: &Path) -> Result<()> {
        if mesh.triangles.len() > u32::MAX as usize {
            return Err("Mesh is too large".into());
        }
        let mut bytes = vec![0u8; 80];
        let header = b"Faber; units are mm ";
        bytes[..header.len()].copy_from_slice(header);
        bytes.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
            let normal = (b - a).cross(c - a).normalize_or_zero();
            for p in [normal, a, b, c] {
                for v in p.to_array() {
                    bytes.extend_from_slice(&(v as f32).to_le_bytes());
                }
            }
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        atomic_write(path, &bytes)
    }
}
