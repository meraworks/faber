use super::{FileStore, atomic_write};
use faber_model::ProjectData;
use faber_ports::{HistoryMove, ProjectStore, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    current: ProjectData,
    undo: Vec<String>,
    redo: Vec<String>,
}

impl FileStore {
    pub(super) fn checkpoint_path(&self, name: &str) -> Result<PathBuf> {
        if name.is_empty()
            || name.len() > 100
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err("Invalid checkpoint name".into());
        }
        Ok(self
            .root
            .join(".faber/checkpoints")
            .join(format!("checkpoint-{}.json", name.to_ascii_lowercase())))
    }
    fn journal_path(&self, revision: u64) -> PathBuf {
        self.root
            .join(".faber/history")
            .join(format!("{revision}.json"))
    }

    fn record(&self, current: &ProjectData) -> Result<Option<Record>> {
        let path = self.journal_path(current.revision);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read history: {e}"))?;
        if record.schema != 1 {
            return Err("Unsupported history schema".into());
        }
        Ok((record.current == *current).then_some(record))
    }

    fn snapshot(&self, data: &ProjectData) -> Result<String> {
        let directory = self.root.join(".faber/history/states");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut file = tempfile::Builder::new()
            .prefix("state-")
            .suffix(".json")
            .tempfile_in(directory)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(data).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        let (_, path) = file.keep().map_err(|e| e.to_string())?;
        Ok(path
            .file_name()
            .ok_or("History snapshot has no name")?
            .to_string_lossy()
            .into_owned())
    }

    fn read_snapshot(&self, name: &str) -> Result<ProjectData> {
        if !name.starts_with("state-")
            || !name.ends_with(".json")
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("Invalid history snapshot name".into());
        }
        let bytes = std::fs::read(self.root.join(".faber/history/states").join(name))
            .map_err(|e| format!("Cannot read history snapshot: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub(super) fn target(&self, action: HistoryMove) -> Result<ProjectData> {
        let current = self.load()?;
        let record = self
            .record(&current)?
            .ok_or("History does not match the current file")?;
        let stack = match action {
            HistoryMove::Undo => &record.undo,
            HistoryMove::Redo => &record.redo,
        };
        self.read_snapshot(stack.last().ok_or("No history step is available")?)
    }

    pub(super) fn commit_history(
        &self,
        expected: Option<&ProjectData>,
        data: &ProjectData,
        action: Option<HistoryMove>,
    ) -> Result<()> {
        if let Some(expected) = expected {
            if self.load()? != *expected {
                return Err("Project changed before save".into());
            }
            if expected.revision.checked_add(1) != Some(data.revision) {
                return Err("Invalid next revision".into());
            }
        } else if self.root.join("faber.json").exists() {
            return Err("Project already exists".into());
        }
        let previous = expected
            .map(|data| self.record(data))
            .transpose()?
            .flatten();
        if action.is_some() && previous.is_none() {
            return Err("History does not match the current file".into());
        }
        let mut record = previous.unwrap_or_else(|| Record {
            schema: 1,
            current: data.clone(),
            undo: vec![],
            redo: vec![],
        });
        if let Some(current) = expected {
            match action {
                None => {
                    record.undo.push(self.snapshot(current)?);
                    record.redo.clear();
                }
                Some(action) => {
                    let name = match action {
                        HistoryMove::Undo => record.undo.pop(),
                        HistoryMove::Redo => record.redo.pop(),
                    }
                    .ok_or("No history step is available")?;
                    let mut target = self.read_snapshot(&name)?;
                    target.revision = data.revision;
                    if target != *data {
                        return Err("History target changed before save".into());
                    }
                    let snapshot = self.snapshot(current)?;
                    match action {
                        HistoryMove::Undo => record.redo.push(snapshot),
                        HistoryMove::Redo => record.undo.push(snapshot),
                    }
                }
            }
        }
        record.current = data.clone();
        atomic_write(
            &self.journal_path(data.revision),
            &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )?;
        if let Some(expected) = expected
            && self.load()? != *expected
        {
            return Err("Project changed before save".into());
        }
        super::layout::write(&self.root, data).map(|_| ())
    }
}
