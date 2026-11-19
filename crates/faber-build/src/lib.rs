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
    #[serde(flatten)]
    authored: Authored,
}

#[derive(Default, Serialize, Deserialize)]
struct Authored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parts: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    drawings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    components: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    configurations: Vec<String>,
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
    drawings: Option<BTreeMap<String, faber_model::drawing::Drawing>>,
    components: Option<BTreeMap<String, faber_model::Component>>,
    configurations: Option<BTreeMap<String, faber_model::Configuration>>,
    studies: Option<BTreeMap<String, faber_model::study::Study<Scalar>>>,
    machining: Option<faber_model::machining::Machining>,
}

/// The project that the source gives. Nothing is written yet.
pub struct Candidate {
    authored: Authored,
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

/// Component frames that came from the last accepted source.
pub fn source_components(root: &Path) -> BTreeSet<String> {
    record(root)
        .map(|r| r.authored.components.into_iter().collect())
        .unwrap_or_default()
}

/// True when the folder has a source and the build record is missing, stale
/// or incomplete.
pub fn needs_build(root: &Path) -> bool {
    source_path(root).is_file() && current_components(root).is_none()
}

/// Return the owned frames if the manifest and the build inputs are current.
pub fn current_components(root: &Path) -> Option<BTreeSet<String>> {
    let record = record(root)?;
    if record.engine != ENGINE || !root.join("faber.json").is_file() {
        return None;
    }
    let changed = record.inputs.iter().any(|input| {
        !std::fs::read(root.join(&input.path)).is_ok_and(|bytes| hash(&bytes) == input.hash)
    });
    (!changed).then(|| record.authored.components.into_iter().collect())
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
    evaluate_with(
        root,
        current,
        current.active_configuration.as_deref(),
        false,
    )
}

/// Evaluate a source with the named configuration. Keep all changes in memory.
pub fn source_data_for(
    root: &Path,
    current: &ProjectData,
    active: Option<&str>,
) -> Result<ProjectData> {
    Ok(evaluate_with(root, current, active, true)?.data)
}

/// Build a selected configuration and record the inputs of that evaluation.
pub fn use_configuration(
    root: &Path,
    project: &mut Project,
    name: &str,
    strict: bool,
) -> Result<()> {
    project.sync()?;
    let candidate = evaluate_with(root, &project.data, Some(name), true)?;
    project.save_generated_checked(candidate.data, strict)?;
    record_project(root, project, &candidate.inputs, candidate.authored)
}

/// Keep a saved configuration consistent with numeric recipes from the source.
pub fn edit_configuration(
    root: &Path,
    project: &mut Project,
    name: String,
    definition: Option<faber_model::Configuration>,
    strict: bool,
) -> Result<()> {
    project.sync()?;
    if record(root).is_some_and(|r| r.authored.configurations.contains(&name)) {
        return Err(format!("Change configuration {name} in {SOURCE_FILE}."));
    }
    if project.data.active_configuration.as_ref() == Some(&name)
        && let Some(config) = &definition
    {
        let mut data = project.data.clone();
        data.configurations.insert(name.clone(), config.clone());
        let candidate = evaluate_with(root, &data, Some(&name), true)?;
        project.save_generated_checked(candidate.data, strict)?;
        return record_project(root, project, &candidate.inputs, candidate.authored);
    }
    let edit = match definition {
        Some(configuration) => faber_model::Edit::SetConfiguration {
            name,
            configuration,
        },
        None => faber_model::Edit::RemoveConfiguration { name },
    };
    project.batch_operations(vec![edit.into()], strict)
}

fn evaluate_with(
    root: &Path,
    current: &ProjectData,
    active: Option<&str>,
    require_active: bool,
) -> Result<Candidate> {
    let src = source_path(root);
    if !src.is_file() {
        return Err(format!("{} has no {SOURCE_FILE}", root.display()));
    }
    let previous = record(root).map(|r| r.authored).unwrap_or_default();
    let fallback = active.and_then(|id| {
        current
            .configurations
            .get(id)
            .filter(|_| !previous.configurations.iter().any(|owned| owned == id))
    });
    let retained = require_active.then_some(&current.params);
    let (value, inputs) = evaluate_source(root, &src, active, fallback, retained)?;
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
        if let faber_model::Geometry::Imported {
            asset,
            purchased: None,
            link: None,
        } = &part.geometry
            && faber_files::study_result::owns(root, &id, asset)?
            && !previous
                .parts
                .as_ref()
                .is_some_and(|owned| owned.contains(&id))
        {
            return Err(format!(
                "Source part {id} uses the id and asset of a saved shape. Use a different id in the source."
            ));
        }
        parts.insert(id, part);
    }
    let source_parts: BTreeSet<String> = parts.keys().cloned().collect();
    for (id, part) in &current.parts {
        if previous
            .parts
            .as_ref()
            .is_some_and(|owned| owned.contains(id))
        {
            continue;
        }
        let faber_model::Geometry::Imported {
            asset,
            purchased: None,
            link: None,
        } = &part.geometry
        else {
            continue;
        };
        if faber_files::study_result::owns(root, id, asset)? {
            if parts.keys().any(|k| k.eq_ignore_ascii_case(id)) {
                return Err(format!(
                    "The source part id {id} conflicts with a saved shape. Use a different id in the source."
                ));
            }
            parts.insert(id.clone(), part.clone());
        }
    }
    let name = design.name.unwrap_or_else(|| current.name.clone());
    let mut data = faber_core::regenerate(current, name, design.params, parts)?;
    // Each new recipe already contains its source suppression state.
    data.configuration_suppressions.clear();
    if let Some(studies) = design.studies {
        data.studies = studies;
    }
    if !require_active
        && current.revision == 0
        && current.parts.is_empty()
        && current.machining.is_empty()
        && let Some(machining) = design.machining
    {
        data.machining = machining;
    }
    let authored = Authored {
        parts: Some(source_parts),
        drawings: replace_authored(&mut data.drawings, previous.drawings, design.drawings),
        components: replace_authored(&mut data.components, previous.components, design.components),
        configurations: replace_authored(
            &mut data.configurations,
            previous.configurations,
            design.configurations,
        ),
    };
    if let Some(id) = active {
        if data.configurations.contains_key(id) {
            faber_core::apply_configuration(&mut data, id)?;
        } else if require_active {
            return Err(format!("Unknown configuration: {id}."));
        } else {
            data.active_configuration = None;
        }
    } else {
        data.active_configuration = None;
    }
    Ok(Candidate {
        data,
        inputs,
        authored,
    })
}

fn replace_authored<T>(
    saved: &mut BTreeMap<String, T>,
    previous: Vec<String>,
    next: Option<BTreeMap<String, T>>,
) -> Vec<String> {
    for id in previous {
        saved.remove(&id);
    }
    let next = next.unwrap_or_default();
    let ids = next.keys().cloned().collect();
    saved.extend(next);
    ids
}

/// Builds the project folder with its own store. Use it when no project is open.
/// A source that does not evaluate writes no file.
pub fn build(kernel: Arc<dyn Kernel>, root: &Path) -> Result<Report> {
    let manifest = root.join("faber.json").is_file();
    let store = Arc::new(FileStore::open(root)?);
    if manifest {
        let mut project =
            Project::open_with_cache(store, kernel, Some(Arc::new(FileAssets::new(root))), None)?;
        return build_project(root, &mut project);
    }
    if root.join("manufacture.json").is_file() {
        return open_saved_manufacture(root, &store, kernel.as_ref());
    }
    let current = empty_project(root);
    let candidate = evaluate(root, &current)?;
    // The first build writes an empty project to open it. A failed build removes that project.
    store.save(None, &current)?;
    let mut project =
        Project::open_with_cache(store, kernel, Some(Arc::new(FileAssets::new(root))), None)?;
    let result = save_candidate(root, &mut project, candidate);
    if result.is_err() {
        for file in ["faber.json", "placement.json"] {
            let _ = std::fs::remove_file(root.join(file));
        }
        let _ = std::fs::remove_file(root.join(".faber/history/0.json"));
    }
    result
}

/// Create a source project around saved manufacturing settings in one store transaction.
pub fn open_saved_manufacture(
    root: &Path,
    store: &FileStore,
    kernel: &dyn Kernel,
) -> Result<Report> {
    let (printer, machining) = faber_files::layout::read_manufacture(root)?;
    let current = empty_project(root);
    let mut candidate = evaluate(root, &current)?;
    candidate.data.machining = machining;
    candidate.data.printer = printer;
    faber_core::build_with_assets(&candidate.data, kernel, Some(&FileAssets::new(root)))?;
    let report = report_of(root, &current, &candidate);
    store.save(None, &candidate.data)?;
    write_record(root, &candidate.inputs, candidate.authored)?;
    Ok(report)
}

/// Saves the project that the source gives. The save is one undo step. The
/// build record is written after the save, so a failed build writes no record.
pub fn build_project(root: &Path, project: &mut Project) -> Result<Report> {
    project.sync()?;
    project.set_source_error(None);
    let candidate = evaluate(root, &project.data)?;
    save_candidate(root, project, candidate)
}

fn save_candidate(root: &Path, project: &mut Project, candidate: Candidate) -> Result<Report> {
    let report = report_of(root, &project.data, &candidate);
    project.save_generated(candidate.data)?;
    record_project(root, project, &candidate.inputs, candidate.authored)?;
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
    write_record(root, &candidate.inputs, candidate.authored)
}

fn record_project(
    root: &Path,
    project: &mut Project,
    inputs: &[PathBuf],
    authored: Authored,
) -> Result<()> {
    project.set_source(Some(SOURCE_FILE.to_owned()));
    project.set_source_components(authored.components.iter().cloned().collect());
    write_record(root, inputs, authored)
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
    for (id, d) in &after.drawings {
        if before.drawings.get(id) != Some(d) {
            files.push(format!("drawings/{id}.json"));
        }
    }
    for id in before
        .drawings
        .keys()
        .filter(|id| !after.drawings.contains_key(*id))
    {
        files.push(format!("drawings/{id}.json"));
    }
    if before.machining != after.machining {
        files.push("manufacture.json".into());
    }
    if before.studies != after.studies {
        files.push("analysis.json".into());
    }
    if before.components != after.components {
        files.push("assembly.json".into());
    }
    if !files.is_empty()
        || before.configurations != after.configurations
        || before.active_configuration != after.active_configuration
        || before.params != after.params
        || before.name != after.name
    {
        files.push("faber.json".into());
    }
    files
}

/// True when two parts have the same recipe and metadata. Placement is not compared.
fn same_recipe(a: &Part<Scalar>, b: &Part<Scalar>) -> bool {
    a.name == b.name
        && a.component == b.component
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
        machining: Default::default(),
        printer: None,
        configurations: Default::default(),
        active_configuration: None,
        configuration_suppressions: Vec::new(),
        components: Default::default(),
        drawings: Default::default(),
        studies: Default::default(),
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

fn write_record(root: &Path, inputs: &[PathBuf], authored: Authored) -> Result<()> {
    let record = BuildRecord {
        schema: RECORD_SCHEMA,
        engine: ENGINE.into(),
        authored,
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

fn evaluate_source(
    root: &Path,
    src: &Path,
    active: Option<&str>,
    fallback: Option<&faber_model::Configuration>,
    retained: Option<&BTreeMap<String, String>>,
) -> Result<(Value, Vec<PathBuf>)> {
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
        let val = if let Some(active) = active {
            // Inherit the source object so a recipe that reads self.params sees the configuration.
            let path = serde_json::to_string(&canonical(src)).map_err(|e| e.to_string())?;
            let id = serde_json::to_string(active).map_err(|e| e.to_string())?;
            let fallback = serde_json::to_string(&fallback).map_err(|e| e.to_string())?;
            let saved = serde_json::to_string(&retained).map_err(|e| e.to_string())?;
            let code = format!(
                r#"local d=import {path};
                local config=std.get(std.get(d,'configurations',{{}}),{id},{fallback});
                local params=std.get(d,'params',{{}});
                local saved={saved};
                local retained=if saved==null then {{}} else {{[k]: saved[k] for k in std.objectFields(saved) if std.objectHas(params,k)}};
                local overrides=if config==null then {{}} else std.get(config,'params',{{}});
                local unknown=[k for k in std.objectFields(overrides) if !std.objectHas(params,k)];
                assert std.length(unknown)==0 : 'Unknown configuration parameters: '+std.join(', ',unknown);
                d + {{ params: params + retained + overrides }}"#
            );
            state
                .evaluate_snippet("configuration", code)
                .map_err(explain)?
        } else {
            state.import(src).map_err(explain)?
        };
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
