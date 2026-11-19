//! Build the selected surface and its inputs for display in the assembly.
pub use crate::history::surface_ops::{creates_surface, is_surface_output, source_geometry};
use crate::{components, history, instances};
use faber_model::{Feature, Geometry, ProjectData};
use faber_ports::{AssetStore, Body, EdgeLines, FeatureCache, Kernel, Result};
use std::collections::BTreeSet;

pub struct Display {
    pub body: Body,
    pub boundaries: EdgeLines,
    pub area_mm2: f64,
}

pub fn display(
    data: &ProjectData,
    part_id: &str,
    feature_id: &str,
    kernel: &dyn Kernel,
    assets: &dyn AssetStore,
    cache: &dyn FeatureCache,
) -> Result<Display> {
    let owner = instances::owner(data, part_id)?;
    let part = &data.parts[part_id];
    let Geometry::History { features, .. } = &data.parts[owner].geometry else {
        return Err(format!("Part {owner} has no feature history."));
    };
    let last = features
        .iter()
        .position(|f| f.id == feature_id)
        .ok_or_else(|| format!("Unknown feature {feature_id} in part {owner}."))?;
    if !history::surface_ops::is_surface_output(features, feature_id) {
        return Err(format!("Feature {feature_id} is not a surface."));
    }
    let mut required = BTreeSet::from([feature_id]);
    let mut selected = Vec::new();
    let mut paths = crate::assets::Paths::new();
    let values = faber_expr::resolve(&data.params)?;
    for feature in features[..=last].iter().rev() {
        if !required.remove(feature.id.as_str()) {
            continue;
        }
        if feature.suppressed {
            required.extend(history::primary_input(&feature.geometry));
        } else {
            required.extend(history::references(&feature.geometry));
            if let Geometry::Imported { asset, .. } = &feature.geometry
                && !paths.contains_key(asset)
            {
                paths.insert(asset.clone(), assets.resolve(asset)?);
            }
        }
        selected.push(Feature {
            id: feature.id.clone(),
            suppressed: feature.suppressed,
            geometry: feature
                .geometry
                .map_numbers(&mut |n| crate::number(n, &values))?,
        });
    }
    selected.reverse();
    let (local, _) = history::build(
        &selected,
        feature_id,
        kernel,
        &mut history::Cache::default(),
        &paths,
        Some(cache),
    )?;
    let placement = components::placement(components::world_placement(data, part_id)?);
    let world = kernel.transform_shape(&local, &placement)?;
    let mesh = kernel.tessellate_faces(&world, crate::TESSELLATION_MM)?;
    let edge_lines = kernel.tessellate_edges(&world, crate::TESSELLATION_MM)?;
    let mut boundaries = EdgeLines::default();
    for id in kernel.open_edge_indices(&world)? {
        let points = edge_lines.edge(id).ok_or("Missing boundary edge.")?;
        let start = boundaries.points.len() as u32;
        boundaries.points.extend_from_slice(points);
        boundaries
            .ranges
            .push(start..boundaries.points.len() as u32);
    }
    let area_mm2 = kernel.surface_area(&world)?;
    let bounds = kernel.properties(&world)?.bounds;
    Ok(Display {
        body: Body {
            id: part_id.to_owned(),
            name: part.name.clone(),
            group: part.group.clone(),
            colour: part.colour,
            mesh: mesh.mesh,
            face_of_triangle: mesh.face_of_triangle,
            edge_lines,
            volume: 0.,
            bounds,
            shape: Some(world),
        },
        boundaries,
        area_mm2,
    })
}
