//! Builds the project of a folder from a modular `design.jsonnet` source.
//!
//! The source is Jsonnet: JSON plus variables, functions and `import`. It
//! evaluates to an object with `parts`, and can have `name` and `params`. An
//! import looks first in the folder of the importing file, then in `lib/` of
//! the project folder. `import 'faber.libsonnet'` is built in.
//!
//! The source owns `faber.json` and the part files. Placement, assembly and
//! analysis stay plain JSON. Each build is a saved change. An undo of a build
//! regenerates the same recipes from the source. `.faber/build.json` records
//! the hash of each input, so [`ensure_built`] builds only when an input changed.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use faber_core::Project;
use faber_files::{FileAssets, FileStore};
use faber_model::{Part, ProjectData, Scalar};
use faber_ports::source::Report;
use faber_ports::{Kernel, ProjectStore, Result};
use jrsonnet_evaluator::error::ErrorKind;
use jrsonnet_evaluator::manifest::JsonFormat;
use jrsonnet_evaluator::trace::{CompactFormat, PathResolver, TraceFormat};
use jrsonnet_evaluator::{
    AsPathLike, FileImportResolver, ImportResolver, SourcePath, SourceVirtual, State,
};
use jrsonnet_gcmodule::Acyclic;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod port;
pub use port::JsonnetSource;

pub const SOURCE_FILE: &str = "design.jsonnet";
const RECORD_FILE: &str = ".faber/build.json";
const HELPER_NAME: &str = "faber.libsonnet";
/// The built-in helper library. Import it as `faber.libsonnet`.
const HELPER: &str = include_str!("faber.libsonnet");
const RECORD_SCHEMA: u32 = 1;
const ENGINE: &str = env!("CARGO_PKG_VERSION");

/// What the last build read.
#[derive(Serialize, Deserialize)]
struct BuildRecord {
    schema: u32,
    engine: String,
    inputs: Vec<Input>,
}

#[derive(Serialize, Deserialize)]
struct Input {
    path: String,
    hash: String,
}

/// The top level of the source. Parts stay as JSON until each one is read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Design {
    name: Option<String>,
    #[serde(default)]
    params: BTreeMap<String, String>,
    parts: BTreeMap<String, Value>,
}

/// The project that the source gives. Nothing is written yet.
pub struct Candidate {
    pub data: ProjectData,
    /// Files that the evaluation read.
    pub inputs: Vec<PathBuf>,
}

fn source_path(root: &Path) -> PathBuf {
    root.join(SOURCE_FILE)
}

fn record(root: &Path) -> Option<BuildRecord> {
    let text = std::fs::read_to_string(root.join(RECORD_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// True when the folder has a build record. A record is kept by the last accepted build.
pub fn has_record(root: &Path) -> bool {
    record(root).is_some()
}

/// True when the folder has a source and the build record is missing, stale
/// or incomplete.
pub fn needs_build(root: &Path) -> bool {
    if !source_path(root).is_file() {
        return false;
    }
    let Some(record) = record(root) else {
        return true;
    };
    if record.engine != ENGINE {
        return true;
    }
    let changed = record.inputs.iter().any(|input| {
        !std::fs::read(root.join(&input.path)).is_ok_and(|bytes| hash(&bytes) == input.hash)
    });
    changed || !root.join("faber.json").is_file()
}

/// Build when an input changed. Return `None` if nothing was built.
pub fn ensure_built(kernel: Arc<dyn Kernel>, root: &Path) -> Result<Option<Report>> {
    if !needs_build(root) {
        return Ok(None);
    }
    build(kernel, root).map(Some)
}

/// Evaluates the source against the current project. Writes no file.
pub fn evaluate(root: &Path, current: &ProjectData) -> Result<Candidate> {
    let src = source_path(root);
    if !src.is_file() {
        return Err(format!("{} has no {SOURCE_FILE}", root.display()));
    }
    let (value, inputs) = evaluate_source(root, &src)?;
    let design: Design =
        serde_json::from_value(value).map_err(|e| format!("{SOURCE_FILE}: {e}"))?;
    let mut parts = BTreeMap::new();
    for (id, value) in design.parts {
        if value.get("placement").is_some() {
            return Err(format!(
                "parts.{id}: placement is not in the source. Move the part with the tools."
            ));
        }
        let part: Part<Scalar> =
            serde_json::from_value(value).map_err(|e| format!("parts.{id}: {e}"))?;
        parts.insert(id, part);
    }
    let name = design.name.unwrap_or_else(|| current.name.clone());
    let data = faber_core::regenerate(current, name, design.params, parts)?;
    Ok(Candidate { data, inputs })
}

/// Builds the project folder with its own store. Use it when no project is open.
/// A source that does not evaluate writes no file.
pub fn build(kernel: Arc<dyn Kernel>, root: &Path) -> Result<Report> {
    let manifest = root.join("faber.json").is_file();
    let store = Arc::new(FileStore::open(root)?);
    let current = if manifest {
        store.load()?
    } else {
        empty_project(root)
    };
    evaluate(root, &current)?;
    if manifest {
        let mut project =
            Project::open_with_cache(store, kernel, Some(Arc::new(FileAssets::new(root))), None)?;
        return build_project(root, &mut project);
    }
    // The first build writes an empty project to open it. A failed build removes that project.
    store.save(None, &current)?;
    let mut project =
        Project::open_with_cache(store, kernel, Some(Arc::new(FileAssets::new(root))), None)?;
    let result = build_project(root, &mut project);
    if result.is_err() {
        for file in ["faber.json", "placement.json"] {
            let _ = std::fs::remove_file(root.join(file));
        }
        let _ = std::fs::remove_file(root.join(".faber/history/0.json"));
    }
    result
}

/// Saves the project that the source gives. The save is one undo step. The
/// build record is written after the save, so a failed build writes no record.
pub fn build_project(root: &Path, project: &mut Project) -> Result<Report> {
    project.sync()?;
    project.set_source_error(None);
    let candidate = evaluate(root, &project.data)?;
    let report = report_of(root, &project.data, &candidate);
    project.save_generated(candidate.data)?;
    write_record(root, &candidate.inputs)?;
    Ok(report)
}

/// Deletes the build record. The next call then builds the source again.
pub fn forget_build(root: &Path) -> Result<()> {
    match std::fs::remove_file(root.join(RECORD_FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot remove the build record: {error}")),
    }
}

/// Writes the build record for the current source. Call it after the project
/// holds the state that the source gives.
pub fn record_build(root: &Path, current: &ProjectData) -> Result<()> {
    let candidate = evaluate(root, current)?;
    write_record(root, &candidate.inputs)
}

fn report_of(root: &Path, before: &ProjectData, candidate: &Candidate) -> Report {
    let after = &candidate.data;
    Report {
        source: SOURCE_FILE.into(),
        written: written_files(before, after),
        parts: after.parts.len(),
        added_parts: after
            .parts
            .keys()
            .filter(|id| !before.parts.contains_key(*id))
            .cloned()
            .collect(),
        removed_parts: before
            .parts
            .keys()
            .filter(|id| !after.parts.contains_key(*id))
            .cloned()
            .collect(),
        warnings: Vec::new(),
        inputs: candidate
            .inputs
            .iter()
            .map(|path| relative(root, path))
            .collect(),
    }
}

/// The files that a save of `after` changes, relative to the project folder.
fn written_files(before: &ProjectData, after: &ProjectData) -> Vec<String> {
    let mut files = Vec::new();
    for (id, part) in &after.parts {
        if !before
            .parts
            .get(id)
            .is_some_and(|old| same_recipe(old, part))
        {
            files.push(format!("parts/{id}.json"));
        }
    }
    for id in before
        .parts
        .keys()
        .filter(|id| !after.parts.contains_key(*id))
    {
        files.push(format!("parts/{id}.json"));
    }
    let placements = |data: &ProjectData| {
        data.parts
            .iter()
            .map(|(id, part)| (id.clone(), part.placement.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    if placements(before) != placements(after) {
        files.push("placement.json".into());
    }
    if !files.is_empty() {
        files.push("faber.json".into());
    }
    files
}

/// True when two parts have the same recipe and metadata. Placement is not compared.
fn same_recipe(a: &Part<Scalar>, b: &Part<Scalar>) -> bool {
    a.name == b.name
        && a.material == b.material
        && a.geometry == b.geometry
        && a.colour == b.colour
        && a.group == b.group
        && a.note == b.note
}

/// The project of a folder with no `faber.json`. It takes the folder name.
fn empty_project(root: &Path) -> ProjectData {
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    ProjectData {
        materials: Default::default(),
        schema: 1,
        revision: 0,
        motion: Default::default(),
        name,
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        sections: Default::default(),
    }
}

fn write_record(root: &Path, inputs: &[PathBuf]) -> Result<()> {
    let record = BuildRecord {
        schema: RECORD_SCHEMA,
        engine: ENGINE.into(),
        inputs: inputs
            .iter()
            .map(|path| Input {
                path: relative(root, path),
                hash: std::fs::read(path)
                    .map(|bytes| hash(&bytes))
                    .unwrap_or_default(),
            })
            .collect(),
    };
    let path = root.join(RECORD_FILE);
    std::fs::create_dir_all(path.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Cannot save {}: {e}", path.display()))
}

/// A starter source for a new project.
fn template(name: &str) -> String {
    format!(
        "// {name}: a Faber design source. Build it with `faber build`.\n{{\n  name: '{name}',\n  parts: {{}},\n}}\n"
    )
}

// ---------------------------------------------------------------- evaluation

#[derive(Acyclic)]
struct Resolver {
    inner: FileImportResolver,
    loaded: Rc<RefCell<BTreeSet<PathBuf>>>,
}

impl ImportResolver for Resolver {
    fn resolve_from(
        &self,
        from: &SourcePath,
        path: &dyn AsPathLike,
    ) -> jrsonnet_evaluator::Result<SourcePath> {
        if path.as_path().to_owned().to_string() == HELPER_NAME {
            return Ok(SourcePath::new(SourceVirtual(HELPER_NAME.into())));
        }
        self.inner.resolve_from(from, path)
    }

    fn load_file_contents(&self, resolved: &SourcePath) -> jrsonnet_evaluator::Result<Vec<u8>> {
        if resolved.downcast_ref::<SourceVirtual>().is_some() {
            return Ok(HELPER.as_bytes().to_vec());
        }
        if let Some(p) = resolved.path() {
            self.loaded.borrow_mut().insert(p.to_path_buf());
        }
        self.inner.load_file_contents(resolved)
    }
}

fn evaluate_source(root: &Path, src: &Path) -> Result<(Value, Vec<PathBuf>)> {
    let loaded = Rc::new(RefCell::new(BTreeSet::new()));
    let resolver = Resolver {
        inner: FileImportResolver::new(vec![root.join("lib")]),
        loaded: loaded.clone(),
    };
    let mut b = State::builder();
    b.import_resolver(resolver)
        .context_initializer(jrsonnet_stdlib::ContextInitializer::new(
            PathResolver::Relative(root.to_path_buf()),
        ));
    let state = b.build();
    let fmt = CompactFormat {
        resolver: PathResolver::Relative(root.to_path_buf()),
        max_trace: 12,
        padding: 2,
    };
    let explain = |e: jrsonnet_evaluator::Error| describe(root, &fmt, &e);
    let text = {
        let _guard = state.enter();
        let val = state.import(src).map_err(explain)?;
        val.manifest(JsonFormat::minify()).map_err(explain)?
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{SOURCE_FILE}: the result is not valid JSON: {e}"))?;
    let inputs = loaded.borrow().iter().cloned().collect();
    Ok((value, inputs))
}

/// The message of an evaluation error. A static error names its file, line and column.
fn describe(root: &Path, fmt: &CompactFormat, error: &jrsonnet_evaluator::Error) -> String {
    let ErrorKind::StaticAnalysisError(diagnostics) = error.error() else {
        return format!(
            "{SOURCE_FILE}: {}",
            fmt.format(error).unwrap_or_else(|_| error.to_string())
        );
    };
    diagnostics
        .iter()
        .map(|d| match &d.span {
            Some(span) => {
                let file = span
                    .0
                    .source_path()
                    .path()
                    .map_or_else(|| SOURCE_FILE.to_string(), |p| relative(root, p));
                let at = span.0.map_source_locations(&[span.1])[0];
                format!("{file}:{}:{}: {}", at.line, at.column, d.message)
            }
            None => format!("{SOURCE_FILE}: {}", d.message),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Canonical path without Windows' `\\?\` prefix.
fn canonical(path: &Path) -> PathBuf {
    let c = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = c.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => c,
    }
}

/// The path of a file inside the project folder, relative and with `/`.
/// A file outside the folder keeps its full path.
fn relative(root: &Path, path: &Path) -> String {
    let path = canonical(path);
    match path.strip_prefix(canonical(root)) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

/// FNV-1a. It is enough to find a changed file.
fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
