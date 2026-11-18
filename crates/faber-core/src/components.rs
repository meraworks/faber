//! Rigid component frames and the parts they contain.
use crate::{euler_from_matrix, rotation_matrix};
use faber_model::{Component, Placement, ProjectData, Scalar};
use faber_ports::Result;
use glam::DAffine3;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn affine(p: &Placement) -> DAffine3 {
    DAffine3::from_mat3_translation(rotation_matrix(p.rotation), p.translation.into())
}
pub(crate) fn placement(a: DAffine3) -> Placement {
    Placement {
        translation: a.translation.to_array(),
        rotation: euler_from_matrix(a.matrix3),
    }
}
pub(crate) type Frames = BTreeMap<String, DAffine3>;
pub(crate) fn frames(data: &ProjectData) -> Result<Frames> {
    let mut done = Frames::new();
    let mut names = BTreeSet::new();
    for (id, c) in &data.components {
        if !crate::valid_id(id)
            || !names.insert(id.to_ascii_lowercase())
            || c.name.trim().is_empty()
        {
            return Err(format!(
                "Component {id} needs a valid unused id and a name."
            ));
        }
        if c.placement
            .translation
            .iter()
            .chain(&c.placement.rotation)
            .any(|n| !n.is_finite() || n.abs() > 1e6)
        {
            return Err(format!(
                "Component {id} placement is outside the supported range."
            ));
        }
        let mut chain = vec![];
        let mut visited = BTreeMap::new();
        let mut current = id;
        let mut transform = loop {
            if let Some(known) = done.get(current) {
                break *known;
            }
            if let Some(&first) = visited.get(current) {
                return Err(format!(
                    "Component {current} is its own parent through: {}.",
                    chain[first..].join(", ")
                ));
            }
            let node = data
                .components
                .get(current)
                .ok_or_else(|| format!("Unknown parent component: {current}."))?;
            visited.insert(current, chain.len());
            chain.push(current.as_str());
            if let Some(parent) = &node.parent {
                current = parent;
            } else {
                break DAffine3::IDENTITY;
            }
        };
        for name in chain.into_iter().rev() {
            transform *= affine(&data.components[name].placement);
            done.insert(name.to_owned(), transform);
        }
    }
    for (id, part) in &data.parts {
        if let Some(c) = &part.component
            && !done.contains_key(c)
        {
            return Err(format!("Part {id} refers to an unknown component: {c}."));
        }
    }
    Ok(done)
}
pub(crate) fn parent_frame(frames: &Frames, component: Option<&str>) -> Result<DAffine3> {
    component.map_or(Ok(DAffine3::IDENTITY), |id| {
        frames
            .get(id)
            .copied()
            .ok_or_else(|| format!("Unknown component: {id}."))
    })
}
pub fn world_placement(data: &ProjectData, id: &str) -> Result<DAffine3> {
    let part = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    let values = faber_expr::resolve(&data.params)?;
    let local = part
        .placement
        .map_numbers(&mut |v| crate::number(v, &values))?;
    Ok(parent_frame(&frames(data)?, part.component.as_deref())? * affine(&local))
}

pub fn component_parts(data: &ProjectData, component: &str, deep: bool) -> Vec<String> {
    if !data.components.contains_key(component) {
        return vec![];
    }
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    if deep {
        for (id, part) in &data.components {
            if let Some(parent) = &part.parent {
                children.entry(parent).or_default().push(id);
            }
        }
    }
    let mut nodes = BTreeSet::new();
    let mut todo = vec![component];
    while let Some(id) = todo.pop() {
        if nodes.insert(id)
            && let Some(list) = children.get(id)
        {
            todo.extend(list);
        }
    }
    data.parts
        .iter()
        .filter(|(_, part)| part.component.as_deref().is_some_and(|p| nodes.contains(p)))
        .map(|(id, _)| id.clone())
        .collect()
}
pub(crate) fn set_component(
    data: &mut ProjectData,
    id: String,
    component: Component,
) -> Result<()> {
    if !crate::valid_id(&id) || component.name.trim().is_empty() {
        return Err(format!("Component {id} needs a valid id and a name."));
    }
    data.components.insert(id, component);
    Ok(())
}
fn scalar(p: Placement) -> Placement<Scalar> {
    Placement {
        translation: p.translation.map(Scalar::Number),
        rotation: p.rotation.map(Scalar::Number),
    }
}
pub fn reparent(data: &mut ProjectData, id: &str, target: Option<String>) -> Result<()> {
    let part = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    if part.component == target {
        return Ok(());
    }
    let parent = parent_frame(&frames(data)?, target.as_deref())?;
    let world = world_placement(data, id)?;
    let part = data.parts.get_mut(id).unwrap();
    part.component = target;
    part.placement = scalar(placement(parent.inverse() * world));
    Ok(())
}
pub(crate) fn remove_component(data: &mut ProjectData, id: &str, keep_parts: bool) -> Result<()> {
    let component = data
        .components
        .get(id)
        .ok_or_else(|| format!("Unknown component: {id}."))?
        .clone();
    let parts = component_parts(data, id, false);
    let children: Vec<_> = data
        .components
        .iter()
        .filter(|(_, c)| c.parent.as_deref() == Some(id))
        .map(|(id, _)| id.clone())
        .collect();
    if !keep_parts && (!parts.is_empty() || !children.is_empty()) {
        return Err(format!(
            "Component {id} contains: {}.",
            parts
                .iter()
                .chain(&children)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let transform = affine(&component.placement);
    let values = faber_expr::resolve(&data.params)?;
    for part in parts {
        let p = data.parts.get_mut(&part).unwrap();
        let local = p
            .placement
            .map_numbers(&mut |v| crate::number(v, &values))?;
        p.placement = scalar(placement(transform * affine(&local)));
        p.component.clone_from(&component.parent);
    }
    for child in children {
        let c = data.components.get_mut(&child).unwrap();
        c.placement = placement(transform * affine(&c.placement));
        c.parent.clone_from(&component.parent);
    }
    data.components.remove(id);
    Ok(())
}

/// Convert an assembled rigid pose back to the part's component frame.
pub fn local_placement(data: &ProjectData, id: &str, world: DAffine3) -> Result<Placement> {
    let part = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    let parent = parent_frame(&frames(data)?, part.component.as_deref())?;
    Ok(placement(parent.inverse() * world))
}
