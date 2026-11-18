//! File links keep the last imported asset when the source is unavailable.
use faber_core::Project;
use faber_model::{Geometry, Link};
use faber_ports::Result;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Current,
    Changed,
    Missing,
}
#[derive(Debug, Serialize)]
pub struct LinkInfo {
    pub part: String,
    pub path: String,
    pub hash: String,
    pub state: LinkState,
}
fn link<'a>(p: &'a Project, id: &str) -> Result<&'a Link> {
    match p.data.parts.get(id).map(|p| &p.geometry) {
        Some(Geometry::Imported { link: Some(l), .. }) => Ok(l),
        Some(_) => Err(format!("Part {id} has no file link.")),
        None => Err(format!("Unknown part: {id}")),
    }
}
fn linked(p: &Project) -> impl Iterator<Item = &String> {
    p.data
        .parts
        .iter()
        .filter(|(_, p)| matches!(p.geometry, Geometry::Imported { link: Some(_), .. }))
        .map(|(id, _)| id)
}
fn source_hash(p: &Project, path: &Path) -> Result<Option<String>> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => p.asset_key(path).map(Some),
        Ok(_) => Err(format!(
            "The linked source is not a file: {}",
            path.display()
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read linked source {}: {e}", path.display())),
    }
}
pub fn list(root: &Path, p: &Project) -> Result<Vec<LinkInfo>> {
    let mut results = vec![];
    let mut hashes = BTreeMap::new();
    for id in linked(p) {
        let l = link(p, id)?;
        let path = root.join(&l.path);
        let hash = match hashes.entry(path) {
            std::collections::btree_map::Entry::Occupied(v) => v.into_mut(),
            std::collections::btree_map::Entry::Vacant(v) => {
                let h = source_hash(p, v.key())?;
                v.insert(h)
            }
        };
        let state = match hash {
            None => LinkState::Missing,
            Some(h) if *h == l.hash => LinkState::Current,
            Some(_) => LinkState::Changed,
        };
        results.push(LinkInfo {
            part: id.clone(),
            path: l.path.clone(),
            hash: l.hash.clone(),
            state,
        });
    }
    Ok(results)
}

fn snapshot(source: &Path, dir: &Path, index: usize) -> Result<PathBuf> {
    if !fs::metadata(source)
        .map_err(|e| format!("Cannot read linked source {}: {e}", source.display()))?
        .is_file()
    {
        return Err("The linked source must be a regular file.".into());
    }
    let mut copy = dir.join(index.to_string());
    if let Some(ext) = source.extension() {
        copy.set_extension(ext);
    }
    fs::copy(source, &copy)
        .map_err(|e| format!("Cannot copy linked source {}: {e}", source.display()))?;
    Ok(copy)
}
pub fn import(
    root: &Path,
    p: &mut Project,
    id: String,
    name: String,
    path: &Path,
    strict: bool,
) -> Result<()> {
    let stored = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_str()
        .ok_or("The link path must be valid text.")?
        .to_string();
    if stored.trim().is_empty() {
        return Err("Supply a file path for the link.".into());
    }
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let file = snapshot(&root.join(path), stage.path(), 0)?;
    p.import_linked(id, name, &file, stored, strict)
}
pub fn refresh(
    root: &Path,
    p: &mut Project,
    parts: Option<&[String]>,
    revision: u64,
) -> Result<Vec<String>> {
    p.check_edit_revision(revision)?;
    let ids: BTreeSet<String> = match parts {
        Some(ids) => ids.iter().cloned().collect(),
        None => linked(p).cloned().collect(),
    };
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut copies = BTreeMap::<PathBuf, (String, PathBuf)>::new();
    let mut updates = BTreeMap::new();
    for id in ids {
        let l = link(p, &id)?;
        let path = root.join(&l.path);
        if let Some((hash, copy)) = copies.get(&path) {
            if *hash != l.hash {
                updates.insert(id, (hash.clone(), copy.clone()));
            }
            continue;
        }
        let Some(hash) = source_hash(p, &path)? else {
            continue;
        };
        if hash == l.hash {
            continue;
        }
        let copy = snapshot(&path, stage.path(), copies.len())?;
        if p.asset_key(&copy)? != hash {
            return Err(format!(
                "The linked source {} changed during refresh. Retry the refresh.",
                l.path
            ));
        }
        copies.insert(path, (hash.clone(), copy.clone()));
        updates.insert(id, (hash, copy));
    }
    let changed = updates.keys().cloned().collect();
    p.refresh_link_assets(revision, updates)?;
    Ok(changed)
}
