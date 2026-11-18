//! Imports an editable project through the local archive reader.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub fn import(archive: &Path, destination: &Path) -> Result<(), String> {
    let archive = archive.canonicalize().map_err(|e| e.to_string())?;
    if !archive.is_file()
        || !archive
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("f3d"))
    {
        return Err("Select a local .f3d archive".into());
    }
    if destination.exists() {
        return Err("The destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let work = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let spool = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("Set LOCALAPPDATA to the local application data folder")?
        .join("Faber/Archive");
    let active = fs::metadata(spool.join("active"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(8));
    if !active {
        return Err("Start the FaberArchive add-in in Fusion before import".into());
    }
    let job = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(spool)
        .map_err(|e| e.to_string())?;
    let pending = job.path().join("pending");
    fs::write(
        &pending,
        serde_json::to_vec(&archive).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(pending, job.path().join("request")).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let reply = loop {
        match fs::read(job.path().join("reply")) {
            Ok(bytes) => break bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        if start.elapsed() > Duration::from_secs(180) {
            return Err("Fusion did not finish the import within 180 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Reply {
        Imported {
            project: Box<faber_model::ProjectData>,
            volume: f64,
            bounds: [[f64; 3]; 2],
        },
        Error(String),
    }
    let (project, volume, bounds) =
        match serde_json::from_slice(&reply).map_err(|e| format!("Invalid archive reply: {e}"))? {
            Reply::Imported {
                project,
                volume,
                bounds,
            } => (project, volume, bounds),
            Reply::Error(message) => return Err(message),
        };
    let recipe = tempfile::NamedTempFile::new_in(work.path()).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(recipe.as_file(), &project).map_err(|e| e.to_string())?;
    recipe.as_file().sync_all().map_err(|e| e.to_string())?;
    let recipe = recipe.into_temp_path();
    fs::copy(&recipe, work.path().join("faber.json")).map_err(|e| e.to_string())?;
    let (_, bodies) = crate::read(work.path())?;
    let [body] = bodies.as_slice() else {
        return Err("The archive and the rebuilt project have different body counts".into());
    };
    if volume <= 0.0
        || (body.volume - volume).abs() > (volume * 0.001).max(0.00001)
        || body
            .bounds
            .iter()
            .flatten()
            .zip(bounds.iter().flatten())
            .any(|(a, b)| (a - b).abs() > 0.01)
    {
        return Err("The rebuilt solid differs from the source volume or bounds".into());
    }
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    if let Err(err) = recipe.persist_noclobber(destination.join("faber.json")) {
        let _ = fs::remove_dir(destination);
        return Err(err.to_string());
    }
    Ok(())
}
