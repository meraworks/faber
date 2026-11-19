//! Record the stock and the immutable mesh of a saved shape study.
use crate::atomic_write;
use faber_ports::{Result, valid_id};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedShape<'a> {
    schema: u32,
    stock: &'a str,
    asset: &'a str,
}

fn path(root: &Path, stock: &str, asset: &str) -> Result<PathBuf> {
    if !valid_id(stock)
        || !valid_id(&format!("{stock}-shape"))
        || asset.len() != 64
        || !asset
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Invalid shape result id or asset hash.".into());
    }
    Ok(root
        .join("study-results")
        .join(format!("{stock}-shape"))
        .join(format!("{asset}.json")))
}

/// A receipt takes effect only when the project contains its exact part and asset.
pub fn record(root: &Path, stock: &str, asset: &str) -> Result<()> {
    let file = path(root, stock, asset)?;
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    for directory in [
        root.join("study-results"),
        file.parent().unwrap().to_owned(),
    ] {
        match std::fs::create_dir(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        if !directory.canonicalize().is_ok_and(|p| p.starts_with(&base)) {
            return Err("The study result folder is outside the project.".into());
        }
    }
    let receipt = SavedShape {
        schema: 1,
        stock,
        asset,
    };
    atomic_write(
        &file,
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
    )
}

pub fn owns(root: &Path, id: &str, asset: &str) -> Result<bool> {
    let Some(stock) = id.strip_suffix("-shape") else {
        return Ok(false);
    };
    if !valid_id(stock) {
        return Ok(false);
    }
    let receipt = path(root, stock, asset)?;
    match std::fs::metadata(&receipt) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
        Ok(m) if m.len() > 1024 => return Err("The saved shape record is too large.".into()),
        Ok(_) => {}
    }
    let bytes = std::fs::read(receipt).map_err(|e| e.to_string())?;
    let saved: SavedShape =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid saved shape record: {e}"))?;
    if saved
        != (SavedShape {
            schema: 1,
            stock,
            asset,
        })
    {
        return Err("The saved shape record does not match its part and asset.".into());
    }
    Ok(true)
}
