//! Group occurrences by source geometry and material.
use faber_model::{Geometry, ProjectData};
use faber_ports::Body;
pub use faber_ports::bom::{BomRow, BomSource};
use std::collections::{BTreeMap, BTreeSet};

pub fn bom(data: &ProjectData, bodies: &[Body], scope: Option<&str>, flat: bool) -> Vec<BomRow> {
    selected(data, bodies.iter(), scope, flat)
}

pub fn selected<'a>(
    data: &ProjectData,
    bodies: impl Iterator<Item = &'a Body>,
    scope: Option<&str>,
    flat: bool,
) -> Vec<BomRow> {
    let include = scope.map(|id| {
        crate::component_parts(data, id, true)
            .into_iter()
            .collect::<BTreeSet<_>>()
    });
    let values = faber_expr::resolve(&data.params).unwrap_or_default();
    let mut recipes = BTreeMap::<&str, String>::new();
    let mut groups = BTreeMap::new();
    for body in bodies {
        if include.as_ref().is_some_and(|ids| !ids.contains(&body.id)) {
            continue;
        }
        let Some(part) = data.parts.get(&body.id) else {
            continue;
        };
        let Ok(source_id) = crate::instances::owner(data, &body.id) else {
            continue;
        };
        let source = &data.parts[source_id];
        let component = if flat { None } else { part.component.clone() };
        let material = crate::part_material(data, &body.id).map(str::to_owned);
        let purchased = match &source.geometry {
            Geometry::Imported { purchased, .. } => purchased.as_deref(),
            _ => None,
        };
        let signature = recipes.entry(source_id).or_insert_with(|| {
            if let Some(p) = purchased {
                return p.designation.clone();
            }
            match source
                .geometry
                .map_numbers(&mut |x| crate::number(x, &values))
            {
                Ok(g) => format!("{g:?}"),
                Err(_) => source_id.to_owned(),
            }
        });
        let key = (
            component.clone(),
            purchased.is_some(),
            signature.clone(),
            material.clone(),
        );
        let mass = purchased.and_then(|p| p.mass_g).or_else(|| {
            let density = crate::materials::get(data, material.as_ref()?)
                .ok()?
                .density_g_cm3;
            let volume = if body.shape.is_some() {
                body.volume
            } else {
                faber_meshtools::try_report(&body.mesh)
                    .ok()?
                    .volume_mm3?
                    .abs()
            };
            Some(density * volume / 1000.)
        });
        let row = groups.entry(key).or_insert_with(|| BomRow {
            item: 0,
            quantity: 0,
            parts: vec![],
            material,
            component,
            mass_g: Some(0.),
            name: source.name.clone(),
            source: purchased.map_or(BomSource::Made, |p| BomSource::Purchased {
                designation: p.designation.clone(),
                source: p.source.clone(),
                licence: p.licence.clone(),
            }),
        });
        row.quantity += 1;
        row.parts.push(body.id.clone());
        row.mass_g = row.mass_g.zip(mass).map(|(a, b)| a + b);
    }
    let mut rows: Vec<_> = groups.into_values().collect();
    for row in &mut rows {
        row.parts.sort();
    }
    rows.sort_by(|a, b| {
        (
            &a.component,
            matches!(a.source, BomSource::Purchased { .. }),
            &a.name,
            &a.parts,
        )
            .cmp(&(
                &b.component,
                matches!(b.source, BomSource::Purchased { .. }),
                &b.name,
                &b.parts,
            ))
    });
    for (i, row) in rows.iter_mut().enumerate() {
        row.item = i as u32 + 1;
    }
    rows
}
