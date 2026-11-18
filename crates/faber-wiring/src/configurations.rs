//! Select a saved configuration or export its resolved bodies.
use crate::{ExportFile, Project, Result};
use faber_model::{Edit, ProjectData};
use std::io;
use std::path::{Path, PathBuf};

pub fn use_configuration(root: &Path, p: &mut Project, name: &str, strict: bool) -> Result<()> {
    if p.source().is_some() {
        faber_build::use_configuration(root, p, name, strict)
    } else {
        let edits = vec![Edit::UseConfiguration {
            name: name.to_owned(),
        }];
        if strict {
            p.batch_strict(edits)
        } else {
            p.batch(edits)
        }
    }
}
pub fn edit(
    root: &Path,
    p: &mut Project,
    name: String,
    definition: Option<faber_model::Configuration>,
    strict: bool,
) -> Result<()> {
    if p.source().is_some() {
        return faber_build::edit_configuration(root, p, name, definition, strict);
    }
    let edit = match definition {
        Some(configuration) => Edit::SetConfiguration {
            name,
            configuration,
        },
        None => Edit::RemoveConfiguration { name },
    };
    p.batch_operations(vec![edit.into()], strict)
}

pub struct ExportOptions<'a> {
    pub format: &'a str,
    pub force: bool,
    pub tolerance: f64,
}
pub fn export(
    root: &Path,
    data: &ProjectData,
    dest: &Path,
    name: Option<&str>,
    all: bool,
    options: &ExportOptions<'_>,
) -> Result<Vec<ExportFile>> {
    let names: Vec<&str> = match (all, name) {
        (true, None) if !data.configurations.is_empty() => {
            data.configurations.keys().map(String::as_str).collect()
        }
        (false, Some(name)) => vec![name],
        _ => return Err("Select one configuration or all saved configurations.".into()),
    };
    // Build all files before replacing earlier exports.
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut files = vec![];
    for name in names {
        if !faber_core::valid_id(name) {
            return Err(format!("Invalid configuration name: {name}."));
        }
        let candidate = if crate::design_source().has_source(root) {
            faber_build::source_data_for(root, data, Some(name))?
        } else {
            let mut candidate = data.clone();
            faber_core::apply_configuration(&mut candidate, name)?;
            candidate
        };
        let project = crate::preview_project(root, candidate)?;
        let folder = if all {
            staging.path().join(name)
        } else {
            staging.path().to_owned()
        };
        files.extend(crate::export_project_with(
            &project.data,
            &project.bodies,
            &folder,
            options.format,
            options.force,
            options.tolerance,
        )?);
    }
    let mut outputs = Vec::with_capacity(files.len());
    for file in &mut files {
        let source = Path::new(&file.path);
        let relative = source
            .strip_prefix(staging.path())
            .map_err(|e| e.to_string())?;
        let target = dest.join(relative);
        outputs.push(
            Output::prepare(source, &target).map_err(|e| format!("{}: {e}", target.display()))?,
        );
        file.path = target.to_string_lossy().into_owned();
    }
    for (index, output) in outputs.iter().enumerate() {
        if let Err(error) = std::fs::rename(output.folder.path().join("new"), &output.target) {
            let mut message = format!("Cannot replace {}: {error}", output.target.display());
            for saved in outputs.drain(..index).rev() {
                if let Err(error) = saved.restore() {
                    message.push_str(&format!(" {error}"));
                }
            }
            return Err(message);
        }
    }
    Ok(files)
}

struct Output {
    target: PathBuf,
    folder: tempfile::TempDir,
    had_file: bool,
}

impl Output {
    fn prepare(source: &Path, target: &Path) -> io::Result<Self> {
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("The export needs a destination folder."))?;
        std::fs::create_dir_all(parent)?;
        let folder = tempfile::tempdir_in(parent)?;
        let had_file = target.try_exists()?;
        if had_file {
            if !target.is_file() {
                return Err(io::Error::other("The output must be a file."));
            }
            let previous = folder.path().join("previous");
            if std::fs::hard_link(target, &previous).is_err() {
                std::fs::copy(target, previous)?;
            }
        }
        std::fs::copy(source, folder.path().join("new"))?;
        Ok(Self {
            target: target.to_owned(),
            folder,
            had_file,
        })
    }

    fn restore(self) -> Result<()> {
        if self.had_file {
            if let Err(error) = std::fs::rename(self.folder.path().join("previous"), &self.target) {
                let previous = self.folder.keep().join("previous");
                return Err(format!(
                    "Restore {} from {}: {error}",
                    self.target.display(),
                    previous.display()
                ));
            }
        } else {
            std::fs::remove_file(&self.target)
                .map_err(|e| format!("Cannot remove {}: {e}", self.target.display()))?;
        }
        Ok(())
    }
}
