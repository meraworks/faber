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

#[derive(Deserialize)]
struct Revision {
    revision: u64,
}

fn read_journal(path: &std::path::Path) -> Result<Record> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read history: {e}"))?;
    if record.schema != 1 {
        return Err("Unsupported history schema".into());
    }
    Ok(record)
}

/// Puts a journal file back as it was. Without an earlier file, removes it.
/// A failure here is ignored. The caller returns the error of the save.
fn restore_journal(journal: &std::path::Path, previous: Option<Vec<u8>>) {
    match previous {
        Some(bytes) => {
            let _ = atomic_write(journal, &bytes);
        }
        None => {
            let _ = std::fs::remove_file(journal);
        }
    }
}

/// True when the files on disk are the old state or a partial write of the new state.
/// The manifest is written last, so it holds the old name, params and revision.
/// Each part and each optional file must equal its old or its new value.
fn continues(disk: &ProjectData, old: &ProjectData, new: &ProjectData) -> bool {
    if disk.revision != old.revision
        || disk.name != old.name
        || disk.params != old.params
        || disk.schema != old.schema
        || disk.materials != old.materials
    {
        return false;
    }
    let parts_ok = old
        .parts
        .keys()
        .chain(new.parts.keys())
        .chain(disk.parts.keys())
        .all(|id| {
            let on_disk = disk.parts.get(id);
            [old.parts.get(id), new.parts.get(id)]
                .into_iter()
                .any(|saved| {
                    if saved == on_disk {
                        return true;
                    }
                    let (Some(saved), Some(actual)) = (saved, on_disk) else {
                        return false;
                    };
                    // Part files contain the recipe. Placement is a separate file.
                    let mut recipe = saved.clone();
                    recipe.placement = actual.placement.clone();
                    &recipe == actual
                })
        });
    // Each drawing has its own file. Other groups share one atomic file.
    let drawings_ok = disk
        .drawings
        .keys()
        .chain(old.drawings.keys())
        .chain(new.drawings.keys())
        .all(|id| {
            disk.drawings.get(id) == old.drawings.get(id)
                || disk.drawings.get(id) == new.drawings.get(id)
        });
    let placements_ok = [old, new].into_iter().any(|source| {
        disk.parts.iter().all(|(id, part)| {
            source.parts.get(id).map_or_else(
                || part.placement == Default::default(),
                |p| p.placement == part.placement,
            )
        })
    });
    parts_ok
        && placements_ok
        && drawings_ok
        && (disk.printer == old.printer || disk.printer == new.printer)
        && ((disk.motion == old.motion && disk.contacts == old.contacts)
            || (disk.motion == new.motion && disk.contacts == new.contacts))
        && ((disk.sections == old.sections && disk.studies == old.studies)
            || (disk.sections == new.sections && disk.studies == new.studies))
}

fn read_snapshot(root: &std::path::Path, name: &str) -> Result<ProjectData> {
    if !name.starts_with("state-")
        || !name.ends_with(".json")
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err("Invalid history snapshot name".into());
    }
    let bytes = std::fs::read(root.join(".faber/history/states").join(name))
        .map_err(|e| format!("Cannot read history snapshot: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// Read one committed revision while an editor can replace several project files.
pub fn read_committed(root: &std::path::Path) -> Result<ProjectData> {
    let manifest =
        std::fs::read(root.join("faber.json")).map_err(|e| format!("Cannot read project: {e}"))?;
    let Revision { revision } =
        serde_json::from_slice(&manifest).map_err(|e| format!("Cannot read faber.json: {e}"))?;
    let disk = super::layout::read(root);
    let Some(next) = revision.checked_add(1) else {
        return disk;
    };
    let journal = root.join(".faber/history");
    let next_path = journal.join(format!("{next}.json"));
    if !next_path.try_exists().map_err(|e| e.to_string())? {
        return disk;
    }
    if disk.as_ref().is_ok_and(|d| d.revision != revision) {
        return Err("The project changed while it was loading.".into());
    }
    let pending = read_journal(&next_path)?;
    if pending.current.revision != next {
        return Err("The pending save has the wrong revision.".into());
    }
    let valid_old = |old: &ProjectData| {
        old.revision == revision
            && disk
                .as_ref()
                .map_or(true, |d| continues(d, old, &pending.current))
    };
    // Normal edits and history moves save their exact starting state. This can include authored changes.
    for name in [pending.undo.last(), pending.redo.last()]
        .into_iter()
        .flatten()
    {
        let old = read_snapshot(root, name)?;
        if valid_old(&old) {
            return Ok(old);
        }
    }
    // A rebuild can retain the undo stacks. Its prior revision stays in the journal.
    if let Ok(old) = read_journal(&journal.join(format!("{revision}.json")))
        && valid_old(&old.current)
    {
        return Ok(old.current);
    }
    Err("The project files do not match the pending save.".into())
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
        let record = read_journal(&path)?;
        Ok((record.current == *current).then_some(record))
    }

    /// Ends a save that stopped after its journal record. The record at revision + 1
    /// holds the new state, and the record at the manifest revision holds the old state.
    /// The save is completed only when the files on disk continue the old state.
    /// Otherwise the files stay as they are, and the record stays.
    pub(super) fn complete_interrupted_save(&self) -> Result<()> {
        if !self.root.join("faber.json").exists() {
            return Ok(());
        }
        let bytes = std::fs::read(self.root.join("faber.json")).map_err(|e| e.to_string())?;
        let Revision { revision } =
            serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read faber.json: {e}"))?;
        let Some(next) = revision.checked_add(1) else {
            return Ok(());
        };
        let next_path = self.journal_path(next);
        if !next_path.exists() {
            return Ok(());
        }
        let Ok(new) = read_journal(&next_path) else {
            return Ok(());
        };
        let Ok(old) = read_journal(&self.journal_path(revision)) else {
            return Ok(());
        };
        if new.current.revision != next || old.current.revision != revision {
            return Ok(());
        }
        let Ok(disk) = super::layout::read(&self.root) else {
            return Ok(());
        };
        if !continues(&disk, &old.current, &new.current) {
            return Ok(());
        }
        super::layout::write(&self.root, &new.current).map(|_| ())
    }

    /// Writes the layout of `data`. When a file write fails, writes `expected` back
    /// and restores the journal record of `data` as it was. Returns the original error.
    fn write_layout(
        &self,
        expected: Option<&ProjectData>,
        data: &ProjectData,
        journal: &std::path::Path,
        previous: Option<Vec<u8>>,
    ) -> Result<()> {
        let Err(error) = super::layout::write(&self.root, data) else {
            return Ok(());
        };
        let restored = match expected {
            Some(old) => super::layout::write(&self.root, old).is_ok(),
            None => true,
        };
        // When the restore fails, the record stays. Open then completes the save
        // when the files are a partial write of it.
        if restored {
            restore_journal(journal, previous);
        }
        Err(error)
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

    pub(super) fn target(&self, action: HistoryMove) -> Result<ProjectData> {
        let current = self.load()?;
        let record = self
            .record(&current)?
            .ok_or("History does not match the current file")?;
        let stack = match action {
            HistoryMove::Undo => &record.undo,
            HistoryMove::Redo => &record.redo,
        };
        read_snapshot(
            &self.root,
            stack.last().ok_or("No history step is available")?,
        )
    }

    /// Writes regenerated parts as the current state. The undo and redo stacks stay unchanged.
    pub(super) fn commit_regenerated(
        &self,
        expected: &ProjectData,
        data: &ProjectData,
    ) -> Result<()> {
        if self.load()? != *expected {
            return Err("Project changed before save".into());
        }
        let mut record = self
            .record(expected)?
            .ok_or("History does not match the current file")?;
        record.current = data.clone();
        let journal = self.journal_path(data.revision);
        let previous = std::fs::read(&journal).ok();
        atomic_write(
            &journal,
            &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )?;
        self.write_layout(Some(expected), data, &journal, previous)
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
                    let mut target = read_snapshot(&self.root, &name)?;
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
        let journal = self.journal_path(data.revision);
        let previous = std::fs::read(&journal).ok();
        atomic_write(
            &journal,
            &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )?;
        let unchanged = match expected {
            Some(expected) => self.load().map(|now| now == *expected),
            None => Ok(true),
        };
        match unchanged {
            Ok(true) => {}
            Ok(false) => {
                restore_journal(&journal, previous);
                return Err("Project changed before save".into());
            }
            Err(error) => {
                restore_journal(&journal, previous);
                return Err(error);
            }
        }
        self.write_layout(expected, data, &journal, previous)
    }
}
