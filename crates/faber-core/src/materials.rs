use crate::Project;
use faber_model::{MaterialClass, MaterialData, ProjectData};
use faber_ports::Result;
use faber_std::materials::{self as library, Class};
use std::collections::BTreeSet;

pub fn ids(data: &ProjectData) -> BTreeSet<&str> {
    library::materials()
        .iter()
        .map(|m| m.id)
        .chain(data.materials.keys().map(String::as_str))
        .collect()
}

pub fn get(data: &ProjectData, id: &str) -> Result<MaterialData> {
    if let Some(m) = data.materials.get(id) {
        return Ok(m.clone());
    }
    let m = library::material(id).ok_or_else(|| {
        crate::names::unknown(
            "material",
            id,
            &ids(data).into_iter().collect::<Vec<_>>(),
            "Read the material list.",
        )
    })?;
    Ok(owned(m))
}

fn owned(m: &library::Material) -> MaterialData {
    MaterialData {
        name: m.name.into(),
        class: match m.class {
            Class::Polymer => MaterialClass::Polymer,
            Class::Metal => MaterialClass::Metal,
        },
        density_g_cm3: m.density_g_cm3,
        youngs_mpa: m.youngs_mpa,
        poisson: m.poisson,
        yield_mpa: m.yield_mpa,
        ultimate_mpa: m.ultimate_mpa,
        conductivity_w_mk: m.conductivity_w_mk,
        expansion_um_mk: m.expansion_um_mk,
        specific_heat_j_kgk: m.specific_heat_j_kgk,
        layer_strength_factor: m.layer_strength_factor,
        colour: m.colour,
        roughness: m.roughness,
        metallic: m.metallic,
        source: m.source.into(),
    }
}

pub fn catalogue(data: Option<&ProjectData>) -> std::collections::BTreeMap<&str, MaterialData> {
    let mut rows: std::collections::BTreeMap<_, _> = library::materials()
        .iter()
        .map(|m| (m.id, owned(m)))
        .collect();
    if let Some(project) = data {
        rows.extend(
            project
                .materials
                .iter()
                .map(|(id, row)| (id.as_str(), row.clone())),
        );
    }
    rows
}

pub(crate) fn validate(data: &ProjectData) -> Result<()> {
    for (id, m) in &data.materials {
        if !crate::valid_id(id) {
            return Err(format!("Invalid material id: {id}"));
        }
        if m.name.trim().is_empty() || m.source.trim().is_empty() {
            return Err(format!("Material {id} needs a name and a source."));
        }
        for (field, v) in [
            ("density", m.density_g_cm3),
            ("Young's modulus", m.youngs_mpa),
            ("yield strength", m.yield_mpa),
            ("ultimate strength", m.ultimate_mpa),
            ("thermal conductivity", m.conductivity_w_mk),
            ("specific heat", m.specific_heat_j_kgk),
        ] {
            if !v.is_finite() || v <= 0. {
                return Err(format!(
                    "Material {id}: {field} must be finite and above zero."
                ));
            }
        }
        if m.yield_mpa > m.ultimate_mpa {
            return Err(format!(
                "Material {id}: yield strength exceeds ultimate strength."
            ));
        }
        if !(m.poisson > -1. && m.poisson < 0.5) {
            return Err(format!(
                "Material {id}: Poisson's ratio must be between -1 and 0.5."
            ));
        }
        if !(m.layer_strength_factor > 0. && m.layer_strength_factor <= 1.) {
            return Err(format!(
                "Material {id}: the layer strength factor must be above zero and at most one."
            ));
        }
        if !m.expansion_um_mk.is_finite()
            || !(0. ..=1.).contains(&m.metallic)
            || !(0. ..=1.).contains(&m.roughness)
        {
            return Err(format!(
                "Material {id}: check the expansion, metallic value and roughness."
            ));
        }
    }
    for part in data.parts.values() {
        if let Some(id) = &part.material
            && !data.materials.contains_key(id)
            && library::material(id).is_none()
        {
            return Err(crate::names::unknown(
                "material",
                id,
                &ids(data).into_iter().collect::<Vec<_>>(),
                "Read the material list.",
            ));
        }
    }
    Ok(())
}

impl Project {
    pub fn set_material(&mut self, id: String, material: MaterialData) -> Result<()> {
        self.sync()?;
        let mut next = self.data.clone();
        next.materials.insert(id, material);
        self.apply(next)
    }

    pub fn remove_material(&mut self, id: &str) -> Result<()> {
        self.sync()?;
        if !self.data.materials.contains_key(id) {
            return Err(format!("Unknown project material: {id}"));
        }
        let users: Vec<_> = self
            .data
            .parts
            .iter()
            .filter(|(_, p)| p.material.as_deref() == Some(id))
            .map(|(name, _)| name.as_str())
            .collect();
        if !users.is_empty() {
            return Err(format!("Material {id} is used by {}.", users.join(", ")));
        }
        let mut next = self.data.clone();
        next.materials.remove(id);
        self.apply(next)
    }
}
