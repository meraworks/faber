use faber_model::ProjectData;
use faber_ports::{Body, ProjectStore};
use std::{collections::BTreeMap, path::Path};

pub fn parameter_user(
    data: &ProjectData,
    values: &BTreeMap<String, String>,
    name: &str,
) -> Option<String> {
    let uses = |text: &str| {
        text.match_indices(name).any(|(i, _)| {
            let identifier = |c: char| c.is_ascii_alphanumeric() || c == '_';
            let suffix = &text[i + name.len()..];
            !text[..i].ends_with(identifier)
                && !suffix.starts_with(identifier)
                && !suffix.trim_start().starts_with('(')
        })
    };
    if let Some((id, _)) = values.iter().find(|(id, expr)| *id != name && uses(expr)) {
        return Some(format!("parameter {id}"));
    }
    let mut check = |s: &faber_model::Scalar| -> Result<f64, String> {
        match s {
            faber_model::Scalar::Expression(e) if uses(e) => Err(String::new()),
            _ => Ok(0.),
        }
    };
    for (id, part) in &data.parts {
        if let faber_model::Geometry::History { features, .. } = &part.geometry {
            for feature in features {
                if feature.geometry.map_numbers(&mut check).is_err() {
                    return Some(format!("{id}/{}", feature.id));
                }
            }
        } else if part.geometry.map_numbers(&mut check).is_err() {
            return Some(id.clone());
        }
        if part.placement.map_numbers(&mut check).is_err() {
            return Some(format!("{id}/placement"));
        }
    }
    data.sections
        .iter()
        .find(|(_, section)| section.map_numbers(&mut check).is_err())
        .map(|(id, _)| format!("section {id}"))
}

pub fn parameters(
    root: &Path,
    expected: &ProjectData,
    values: BTreeMap<String, String>,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let (mut project, store) = super::open_with_store(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the parameter editor.".into());
    }
    if let Some(source) = project.source() {
        return Err(format!(
            "This project is generated from {source}. Edit the source, then call build."
        ));
    }
    let removed = expected
        .params
        .keys()
        .filter(|k| !values.contains_key(*k))
        .collect::<Vec<_>>();
    for name in &removed {
        if let Some(owner) = parameter_user(expected, &values, name) {
            return Err(format!("Parameter {name} is used by {owner}."));
        }
    }
    if values == expected.params {
        return Ok((project.data, project.bodies));
    }
    let updates = values
        .iter()
        .filter(|(k, v)| expected.params.get(*k) != Some(*v))
        .map(|(name, expr)| faber_model::Edit::SetParam {
            name: name.clone(),
            expression: expr.clone(),
        })
        .collect::<Vec<_>>();
    let (mut candidate, bodies) = if updates.is_empty() {
        let mut candidate = expected.clone();
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or("Revision limit reached")?;
        (candidate, project.bodies)
    } else {
        project.preview_batch(updates, strict)?
    };
    candidate.params = values;
    store.save_checked(expected, &candidate)?;
    Ok((candidate, bodies))
}
