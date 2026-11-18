//! Named parameter values and feature suppression.
use faber_model::{Configuration, Feature, Geometry, ProjectData, Scalar, SuppressRef};
use faber_ports::Result;
use std::collections::BTreeSet;

fn feature<'a>(data: &'a mut ProjectData, item: &SuppressRef) -> Result<&'a mut Feature<Scalar>> {
    let Some(Geometry::History { features, .. }) =
        data.parts.get_mut(&item.part).map(|p| &mut p.geometry)
    else {
        return Err(format!("Part {} needs a feature history.", item.part));
    };
    features
        .iter_mut()
        .find(|f| f.id == item.feature)
        .ok_or_else(|| format!("Unknown feature {} in part {}.", item.feature, item.part))
}
fn check(data: &ProjectData, name: &str, config: &Configuration) -> Result<()> {
    if !crate::valid_id(name)
        || data
            .configurations
            .keys()
            .any(|id| id != name && id.eq_ignore_ascii_case(name))
    {
        return Err(format!("Invalid configuration name: {name}."));
    }
    for param in config.params.keys() {
        if !data.params.contains_key(param) {
            return Err(format!("Unknown parameter: {param}."));
        }
    }
    if !config.params.is_empty() {
        let mut expressions = data.params.clone();
        expressions.extend(config.params.clone());
        faber_expr::resolve(&expressions)?;
    }
    let mut found = BTreeSet::new();
    for item in &config.suppressed {
        let exists = data.parts.get(&item.part).is_some_and(|part| {
            matches!(&part.geometry, Geometry::History { features, .. }
                if features.iter().any(|feature| feature.id == item.feature))
        });
        if !exists || !found.insert(item) {
            return Err(format!(
                "Unknown or repeated feature {} in part {}.",
                item.feature, item.part
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate(data: &ProjectData) -> Result<()> {
    for (name, config) in &data.configurations {
        check(data, name, config)?;
    }
    if let Some(active) = &data.active_configuration
        && !data.configurations.contains_key(active)
    {
        return Err(format!("Unknown active configuration: {active}."));
    }
    Ok(())
}
pub(crate) fn set(data: &mut ProjectData, name: String, config: Configuration) -> Result<()> {
    check(data, &name, &config)?;
    data.configurations.insert(name.clone(), config);
    if data.active_configuration.as_deref() == Some(&name) {
        apply_configuration(data, &name)?;
    }
    Ok(())
}
fn clear(data: &mut ProjectData) {
    for item in std::mem::take(&mut data.configuration_suppressions) {
        if let Ok(f) = feature(data, &item) {
            f.suppressed = false;
        }
    }
    data.active_configuration = None;
}
pub(crate) fn remove(data: &mut ProjectData, name: &str) -> Result<()> {
    data.configurations
        .remove(name)
        .ok_or_else(|| format!("Unknown configuration: {name}."))?;
    if data.active_configuration.as_deref() == Some(name) {
        clear(data);
    }
    Ok(())
}
/// Set the named parameters and suppress the selected features in a candidate design.
pub fn apply_configuration(data: &mut ProjectData, name: &str) -> Result<()> {
    let config = data
        .configurations
        .get(name)
        .ok_or_else(|| format!("Unknown configuration: {name}."))?
        .clone();
    check(data, name, &config)?;
    let mut params = data.params.clone();
    params.extend(config.params);
    faber_expr::resolve(&params)?;
    clear(data);
    data.params = params;
    for item in config.suppressed {
        let f = feature(data, &item)?;
        if !f.suppressed {
            f.suppressed = true;
            data.configuration_suppressions.push(item);
        }
    }
    data.active_configuration = Some(name.to_owned());
    Ok(())
}
