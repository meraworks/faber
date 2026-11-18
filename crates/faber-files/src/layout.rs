//! The project folder layout. A manifest, one file for each part, and separate
//! files for placement, assembly and analysis.
//!
//! `faber.json`: schema 2, revision, name, params.
//! `parts/<id>.json`: schema 1, the recipe of one part. No placement.
//! `placement.json`: schema 1, the placement of each part.
//! `assembly.json`: schema 1, motion and contacts. Absent when empty.
//! `analysis.json`: schema 1, saved sections and studies. Absent when empty.
//!
//! A schema 1 `faber.json` holds the whole project in one file. It still opens.

use super::atomic_write;
use faber_model::drawing::Drawing;
use faber_model::motion::MotionStudy;
use faber_model::{Contact, Geometry, Part, Placement, ProjectData, Scalar, SectionAnalysis};
use faber_ports::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MANIFEST_SCHEMA: u32 = 2;
const FILE_SCHEMA: u32 = 1;
const ONE_FILE_SCHEMA: u32 = 1;
const OPTIONAL_FILES: [&str; 4] = [
    "placement.json",
    "assembly.json",
    "analysis.json",
    "manufacture.json",
];

#[derive(Deserialize)]
struct Probe {
    schema: u32,
}

#[derive(Serialize, Deserialize)]
struct DrawingFile {
    schema: u32,
    #[serde(flatten)]
    drawing: Drawing,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManufactureFile {
    schema: u32,
    printer: faber_model::Printer,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    materials: BTreeMap<String, faber_model::MaterialData>,
    schema: u32,
    revision: u64,
    name: String,
    params: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<String>,
    schema: u32,
    name: String,
    geometry: Geometry<Scalar>,
    colour: [u8; 3],
    #[serde(default)]
    group: String,
    #[serde(default)]
    note: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementFile {
    schema: u32,
    parts: BTreeMap<String, Placement<Scalar>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssemblyFile {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    components: BTreeMap<String, faber_model::Component>,
    schema: u32,
    #[serde(default)]
    motion: MotionStudy,
    #[serde(default)]
    contacts: BTreeMap<String, Contact>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisFile {
    schema: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    studies: BTreeMap<String, faber_model::study::Study<Scalar>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sections: BTreeMap<String, SectionAnalysis<Scalar>>,
}

/// Reads a project folder. Accepts schema 1 (one file) and schema 2 (split).
pub fn read(root: &Path) -> Result<ProjectData> {
    let bytes =
        std::fs::read(root.join("faber.json")).map_err(|e| format!("Cannot read project: {e}"))?;
    let schema = serde_json::from_slice::<Probe>(&bytes)
        .map_err(|e| format!("Cannot read faber.json: {e}"))?
        .schema;
    match schema {
        ONE_FILE_SCHEMA => {
            serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read project data: {e}"))
        }
        MANIFEST_SCHEMA => read_split(root, &bytes),
        other => Err(format!(
            "Unsupported project schema {other}. Update Faber to open this project."
        )),
    }
}

fn read_split(root: &Path, manifest_bytes: &[u8]) -> Result<ProjectData> {
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| format!("Cannot read faber.json: {e}"))?;
    let mut placements = match read_optional::<PlacementFile>(root, "placement.json")? {
        Some(file) => {
            check_schema(&file.schema, "placement.json")?;
            file.parts
        }
        None => BTreeMap::new(),
    };
    let mut parts = BTreeMap::new();
    for id in file_ids(root, "parts")? {
        if !faber_ports::valid_id(&id) {
            return Err(format!("Invalid part file name: parts/{id}.json"));
        }
        let name = format!("parts/{id}.json");
        let file: PartFile = read_json(root, &name)?;
        check_schema(&file.schema, &name)?;
        let placement = placements.remove(&id).unwrap_or_default();
        parts.insert(
            id,
            Part {
                component: file.component,
                material: file.material,
                name: file.name,
                geometry: file.geometry,
                placement,
                colour: file.colour,
                group: file.group,
                note: file.note,
            },
        );
    }
    if let Some(id) = placements.keys().next() {
        return Err(format!("placement.json places an unknown part: {id}"));
    }
    let (motion, contacts, components) = match read_optional::<AssemblyFile>(root, "assembly.json")?
    {
        Some(file) => {
            check_schema(&file.schema, "assembly.json")?;
            (file.motion, file.contacts, file.components)
        }
        None => Default::default(),
    };
    let (sections, studies) = match read_optional::<AnalysisFile>(root, "analysis.json")? {
        Some(file) => {
            check_schema(&file.schema, "analysis.json")?;
            (file.sections, file.studies)
        }
        None => Default::default(),
    };
    let printer = match read_optional::<ManufactureFile>(root, "manufacture.json")? {
        Some(file) => {
            check_schema(&file.schema, "manufacture.json")?;
            Some(file.printer)
        }
        None => None,
    };
    let mut drawings = BTreeMap::new();
    for id in file_ids(root, "drawings")? {
        let name = format!("drawings/{id}.json");
        let file: DrawingFile = read_json(root, &name)?;
        check_schema(&file.schema, &name)?;
        drawings.insert(id, file.drawing);
    }
    check_drawings(&drawings)?;
    Ok(ProjectData {
        components,
        drawings,
        printer,
        materials: manifest.materials,
        studies,
        schema: ONE_FILE_SCHEMA,
        revision: manifest.revision,
        motion,
        name: manifest.name,
        parts,
        params: manifest.params,
        contacts,
        sections,
    })
}

/// Writes the split layout. Writes only files whose bytes change. Removes
/// part files of removed parts. Writes faber.json last.
pub fn write(root: &Path, data: &ProjectData) -> Result<Vec<PathBuf>> {
    if let Some(printer) = &data.printer {
        printer.validate()?;
    }
    if let Some(id) = data.parts.keys().find(|id| !faber_ports::valid_id(id)) {
        return Err(format!("Invalid part name for a file: {id}"));
    }
    check_drawings(&data.drawings)?;
    let mut changed = Vec::new();
    for (id, drawing) in &data.drawings {
        let file = DrawingFile {
            schema: FILE_SCHEMA,
            drawing: drawing.clone(),
        };
        put(root, &format!("drawings/{id}.json"), &file, &mut changed)?;
    }
    for id in file_ids(root, "drawings")? {
        if !data
            .drawings
            .keys()
            .any(|key| key.eq_ignore_ascii_case(&id))
        {
            remove(root.join(format!("drawings/{id}.json")), &mut changed)?;
        }
    }
    for (id, part) in &data.parts {
        let file = PartFile {
            component: part.component.clone(),
            material: part.material.clone(),
            schema: FILE_SCHEMA,
            name: part.name.clone(),
            geometry: part.geometry.clone(),
            colour: part.colour,
            group: part.group.clone(),
            note: part.note.clone(),
        };
        put(root, &format!("parts/{id}.json"), &file, &mut changed)?;
    }
    let placement = PlacementFile {
        schema: FILE_SCHEMA,
        parts: data
            .parts
            .iter()
            .map(|(id, part)| (id.clone(), part.placement.clone()))
            .collect(),
    };
    put(root, "placement.json", &placement, &mut changed)?;
    if data.motion == MotionStudy::default()
        && data.contacts.is_empty()
        && data.components.is_empty()
    {
        remove(root.join("assembly.json"), &mut changed)?;
    } else {
        let file = AssemblyFile {
            components: data.components.clone(),
            schema: FILE_SCHEMA,
            motion: data.motion.clone(),
            contacts: data.contacts.clone(),
        };
        put(root, "assembly.json", &file, &mut changed)?;
    }
    if data.sections.is_empty() && data.studies.is_empty() {
        remove(root.join("analysis.json"), &mut changed)?;
    } else {
        let file = AnalysisFile {
            schema: FILE_SCHEMA,
            sections: data.sections.clone(),
            studies: data.studies.clone(),
        };
        put(root, "analysis.json", &file, &mut changed)?;
    }
    if let Some(printer) = &data.printer {
        let file = ManufactureFile {
            schema: FILE_SCHEMA,
            printer: printer.clone(),
        };
        put(root, "manufacture.json", &file, &mut changed)?;
    } else {
        remove(root.join("manufacture.json"), &mut changed)?;
    }
    // Last, after the placement. A part file without a placement entry reads with
    // the identity placement, so a stop before this point still reads.
    for id in file_ids(root, "parts")? {
        // On a case-insensitive file system a stale name can be the live file of another id.
        let live = data.parts.keys().any(|key| key.eq_ignore_ascii_case(&id));
        if !live {
            remove(root.join(format!("parts/{id}.json")), &mut changed)?;
        }
    }
    let manifest = Manifest {
        materials: data.materials.clone(),
        schema: MANIFEST_SCHEMA,
        revision: data.revision,
        name: data.name.clone(),
        params: data.params.clone(),
    };
    put(root, "faber.json", &manifest, &mut changed)?;
    Ok(changed)
}

/// A value that changes when a layout file changes (newest mtime and total length).
pub fn fingerprint(root: &Path) -> Result<(SystemTime, u64)> {
    let mut newest = SystemTime::UNIX_EPOCH;
    let mut total = 0;
    let mut files = vec![root.join("faber.json")];
    files.extend(
        OPTIONAL_FILES
            .iter()
            .map(|name| root.join(name))
            .filter(|path| path.exists()),
    );
    for id in file_ids(root, "parts")? {
        files.push(root.join(format!("parts/{id}.json")));
    }
    for id in file_ids(root, "drawings")? {
        files.push(root.join(format!("drawings/{id}.json")));
    }
    for path in files {
        let metadata =
            std::fs::metadata(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let modified = metadata.modified().map_err(|e| e.to_string())?;
        newest = newest.max(modified);
        total += metadata.len();
    }
    Ok((newest, total))
}

/// The ids of the data files in a folder, sorted.
fn file_ids(root: &Path, folder: &str) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(root.join(folder)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Cannot read {folder}: {e}")),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && let Some(stem) = path.file_stem()
        {
            ids.push(stem.to_string_lossy().into_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

fn check_drawings(drawings: &BTreeMap<String, Drawing>) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for (id, drawing) in drawings {
        if !faber_ports::valid_id(id) || !seen.insert(id.to_ascii_lowercase()) {
            return Err(format!("Invalid or duplicate drawing name: {id}"));
        }
        drawing.validate()?;
    }
    Ok(())
}

fn check_schema(schema: &u32, name: &str) -> Result<()> {
    if *schema != FILE_SCHEMA {
        return Err(format!("Unsupported schema {schema} in {name}"));
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(root: &Path, name: &str) -> Result<T> {
    let bytes = std::fs::read(root.join(name)).map_err(|e| format!("Cannot read {name}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read {name}: {e}"))
}

fn read_optional<T: DeserializeOwned>(root: &Path, name: &str) -> Result<Option<T>> {
    if !root.join(name).exists() {
        return Ok(None);
    }
    read_json(root, name).map(Some)
}

fn put<T: Serialize + DeserializeOwned>(
    root: &Path,
    name: &str,
    value: &T,
    changed: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let path = root.join(name);
    if has_exact_name(&path)
        && let Ok(current) = std::fs::read(&path)
    {
        if current == bytes {
            return Ok(());
        }
        // Keep the original bytes when only layout or default fields differ.
        if let Ok(old) = serde_json::from_slice::<T>(&current)
            && serde_json::to_vec_pretty(&old).is_ok_and(|old| old == bytes[..bytes.len() - 1])
        {
            return Ok(());
        }
    }
    atomic_write(&path, &bytes)?;
    changed.push(path);
    Ok(())
}

/// True when the file exists under exactly this name. A case-insensitive file
/// system finds `Lid.json` for `lid.json`, so the name must match too.
fn has_exact_name(path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    std::fs::read_dir(parent)
        .is_ok_and(|entries| entries.flatten().any(|entry| entry.file_name() == name))
}

fn remove(path: PathBuf, changed: &mut Vec<PathBuf>) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Cannot remove {}: {e}", path.display()))?;
        changed.push(path);
    }
    Ok(())
}
