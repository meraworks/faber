//! Project operations through ports.

mod assets;
mod batch;
mod feature_cache;
mod feature_edit;
mod history;
mod hole;
mod pattern;
mod provenance;
mod section;
mod select;

pub use batch::{Operation, PartInfo, PartInfoOperation};
use faber_model::{Part, ProjectData, Scalar};
use faber_ports::{Body, Kernel, ProjectStore, Result};
use glam::{DMat3, DVec3};
pub use section::{CutPart, ResolvedSection, SectionReport};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Mass properties of a placed part in world coordinates.
pub struct MassReport {
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub centre_of_mass_mm: [f64; 3],
    /// Present only when a density is given.
    pub mass_g: Option<f64>,
    /// Present only when a density is given.
    pub inertia_kg_mm2: Option<[[f64; 3]; 3]>,
}

pub struct Project {
    pub data: ProjectData,
    pub bodies: Vec<Body>,
    kernel: Arc<dyn Kernel>,
    store: Arc<dyn ProjectStore>,
    histories: BTreeMap<String, history::Cache>,
    checks: Option<Arc<dyn faber_ports::DesignChecks>>,
    assets: Option<Arc<dyn faber_ports::AssetStore>>,
    cache: Option<Arc<dyn faber_ports::FeatureCache>>,
}

struct Prepared {
    data: ProjectData,
    bodies: Vec<Body>,
    histories: BTreeMap<String, history::Cache>,
}

impl Project {
    pub fn with_checks(mut self, checks: Arc<dyn faber_ports::DesignChecks>) -> Self {
        self.checks = Some(checks);
        self
    }
    pub fn open(store: Arc<dyn ProjectStore>, kernel: Arc<dyn Kernel>) -> Result<Self> {
        Self::open_with_assets(store, kernel, None)
    }

    pub fn open_with_assets(
        store: Arc<dyn ProjectStore>,
        kernel: Arc<dyn Kernel>,
        assets: Option<Arc<dyn faber_ports::AssetStore>>,
    ) -> Result<Self> {
        Self::open_with_cache(store, kernel, assets, None)
    }

    /// Opens a project. Built features also go to `cache` when the kernel has a cache version.
    pub fn open_with_cache(
        store: Arc<dyn ProjectStore>,
        kernel: Arc<dyn Kernel>,
        assets: Option<Arc<dyn faber_ports::AssetStore>>,
        cache: Option<Arc<dyn faber_ports::FeatureCache>>,
    ) -> Result<Self> {
        let data = store.load()?;
        let mut histories = BTreeMap::new();
        let bodies = build_cached(
            &data,
            kernel.as_ref(),
            &mut histories,
            assets.as_deref(),
            cache.as_deref(),
        )?;
        Ok(Self {
            data,
            bodies,
            kernel,
            store,
            histories,
            checks: None,
            assets,
            cache,
        })
    }

    pub fn sync(&mut self) -> Result<()> {
        let data = self.store.load()?;
        assets::resolve(&data, self.assets.as_deref())?;
        if data != self.data {
            let mut histories = self.histories.clone();
            let bodies = build_cached(
                &data,
                self.kernel.as_ref(),
                &mut histories,
                self.assets.as_deref(),
                self.cache.as_deref(),
            )?;
            self.data = data;
            self.bodies = bodies;
            self.histories = histories;
        }
        Ok(())
    }

    pub fn put_part<N: Clone + Into<Scalar>>(&mut self, id: String, part: Part<N>) -> Result<()> {
        self.put_parts(BTreeMap::from([(id, part)]))
    }

    pub fn import_part(
        &mut self,
        id: String,
        name: String,
        source: &std::path::Path,
        strict: bool,
    ) -> Result<()> {
        self.sync()?;
        if !valid_id(&id)
            || self
                .data
                .parts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(&id))
        {
            return Err("Import needs a new valid part id".into());
        }
        let asset = self
            .assets
            .as_ref()
            .ok_or("Asset storage is not available")?
            .put(source)?;
        let part = Part {
            name,
            group: String::new(),
            note: String::new(),
            colour: [100, 140, 170],
            placement: Default::default(),
            geometry: faber_model::Geometry::<Scalar>::Imported { asset },
        };
        let operations = vec![faber_model::Edit::PutPart { id, part }];
        if strict {
            self.batch_strict(operations)
        } else {
            self.batch(operations)
        }
    }

    pub fn put_parts<N: Clone + Into<Scalar>>(
        &mut self,
        parts: BTreeMap<String, Part<N>>,
    ) -> Result<()> {
        self.sync()?;
        let mut next = self.data.clone();
        for (id, part) in parts {
            next.parts
                .insert(id, part.map_numbers(|n| Ok(n.clone().into()))?);
        }
        self.apply(next)
    }

    /// Change the name, colour, group or note of a part. The solid does not rebuild.
    pub fn set_part_info(&mut self, info: PartInfo) -> Result<()> {
        self.batch_operations(
            vec![Operation::SetPartInfo(PartInfoOperation::SetPartInfo(info))],
            false,
        )
    }

    pub fn set_params(&mut self, params: BTreeMap<String, String>) -> Result<()> {
        self.sync()?;
        let mut next = self.data.clone();
        next.params.extend(params);
        self.apply(next)
    }

    pub fn params(&self) -> Result<BTreeMap<String, f64>> {
        faber_expr::resolve(&self.data.params)
    }

    pub fn topology(
        &self,
        id: &str,
    ) -> Result<(Vec<faber_ports::EdgeInfo>, Vec<faber_ports::FaceInfo>)> {
        let body = self
            .bodies
            .iter()
            .find(|body| body.id == id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        let shape = body.shape.as_ref().ok_or("Part has no exact shape")?;
        Ok((self.kernel.edges(shape)?, self.kernel.faces(shape)?))
    }

    pub fn measure_distance(&self, a: &str, b: &str) -> Result<faber_ports::Distance> {
        let shape = |id: &str| {
            self.bodies
                .iter()
                .find(|body| body.id == id)
                .ok_or_else(|| format!("Unknown part: {id}"))?
                .shape
                .as_ref()
                .ok_or_else(|| format!("Part {id} has no exact shape"))
        };
        self.kernel.distance(shape(a)?, shape(b)?)
    }

    pub fn mass_properties(&self, id: &str, density_g_cm3: Option<f64>) -> Result<MassReport> {
        if let Some(density) = density_g_cm3
            && !(density.is_finite() && density > 0.)
        {
            return Err("Density must be above zero".into());
        }
        let shape = self
            .bodies
            .iter()
            .find(|body| body.id == id)
            .ok_or_else(|| format!("Unknown part: {id}"))?
            .shape
            .as_ref()
            .ok_or_else(|| format!("Part {id} has no exact shape"))?;
        let properties = self.kernel.mass_properties(shape)?;
        // One gram per cubic centimetre is 1e-3 g/mm3 or 1e-6 kg/mm3.
        let mass = density_g_cm3.map(|density| {
            let kg_per_mm3 = density * 1e-6;
            (
                density * properties.volume_mm3 * 1e-3,
                properties
                    .inertia_unit_mm5
                    .map(|row| row.map(|v| v * kg_per_mm3)),
            )
        });
        Ok(MassReport {
            volume_mm3: properties.volume_mm3,
            area_mm2: properties.area_mm2,
            centre_of_mass_mm: properties.centre_of_mass_mm,
            mass_g: mass.map(|(grams, _)| grams),
            inertia_kg_mm2: mass.map(|(_, inertia)| inertia),
        })
    }

    pub fn feature_topology(
        &self,
        id: &str,
        feature: &str,
    ) -> Result<(Vec<faber_ports::EdgeInfo>, Vec<faber_ports::FaceInfo>)> {
        let shape = self.feature_shape(id, feature)?;
        Ok((self.kernel.edges(shape)?, self.kernel.faces(shape)?))
    }

    /// Edges and faces of a part or one feature. Each selector keeps only the items that pass it.
    /// A selector count is not checked here.
    pub fn selected_topology(
        &self,
        id: &str,
        feature: Option<&str>,
        edge: Option<&faber_model::EdgeSelector>,
        face: Option<&faber_model::FaceSelector>,
    ) -> Result<(Vec<faber_ports::EdgeInfo>, Vec<faber_ports::FaceInfo>)> {
        let shape = match feature {
            Some(feature) => self.feature_shape(id, feature)?,
            None => {
                let body = self
                    .bodies
                    .iter()
                    .find(|body| body.id == id)
                    .ok_or_else(|| format!("Unknown part: {id}"))?;
                body.shape.as_ref().ok_or("Part has no exact shape")?
            }
        };
        let kernel = self.kernel.as_ref();
        let edges = kernel.edges(shape)?;
        let faces = kernel.faces(shape)?;
        let edges = match edge {
            Some(selector) => select::filter_edges(kernel, shape, edges, selector)?,
            None => edges,
        };
        let faces = match face {
            Some(selector) => select::filter_faces(shape, faces, selector)?,
            None => faces,
        };
        Ok((edges, faces))
    }

    fn feature_shape(&self, id: &str, feature: &str) -> Result<&faber_ports::Shape> {
        let part = self
            .data
            .parts
            .get(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        let faber_model::Geometry::History { features, .. } = &part.geometry else {
            return Err(format!("Part {id} has no feature history"));
        };
        if !features.iter().any(|entry| entry.id == feature) {
            return Err(format!("Unknown feature {feature} in part {id}"));
        }
        self.histories
            .get(id)
            .and_then(|cache| cache.shape(feature))
            .ok_or_else(|| format!("Unknown feature {feature} in part {id}"))
    }

    pub fn remove_part(&mut self, id: &str) -> Result<()> {
        self.sync()?;
        let mut next = self.data.clone();
        next.parts
            .remove(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        next.contacts.retain(|_, c| c.a != id && c.b != id);
        section::detach_part(&mut next, id)?;
        self.apply(next)
    }

    fn apply(&mut self, next: ProjectData) -> Result<()> {
        self.apply_history(next, None, false)
    }

    pub fn undo(&mut self) -> Result<()> {
        self.move_history(faber_ports::HistoryMove::Undo)
    }
    pub fn checkpoint(&mut self, name: &str) -> Result<()> {
        self.sync()?;
        if !valid_id(name) {
            return Err("Invalid checkpoint name".into());
        }
        self.store.checkpoint(name, &self.data)
    }

    pub fn restore(&mut self, name: &str) -> Result<()> {
        self.sync()?;
        if !valid_id(name) {
            return Err("Invalid checkpoint name".into());
        }
        let target = self.store.read_checkpoint(name)?;
        self.apply(target)
    }
    pub fn redo(&mut self) -> Result<()> {
        self.move_history(faber_ports::HistoryMove::Redo)
    }

    fn move_history(&mut self, action: faber_ports::HistoryMove) -> Result<()> {
        self.sync()?;
        let target = self.store.history_target(action)?;
        self.apply_history(target, Some(action), false)
    }

    fn apply_history(
        &mut self,
        next: ProjectData,
        action: Option<faber_ports::HistoryMove>,
        strict: bool,
    ) -> Result<()> {
        let Prepared {
            data: next,
            bodies,
            histories,
        } = self.prepare(next, strict)?;
        match action {
            Some(action) => self.store.save_history(&self.data, &next, action)?,
            None => self.store.save_checked(&self.data, &next)?,
        }
        self.data = next;
        self.bodies = bodies;
        self.histories = histories;
        Ok(())
    }

    fn prepare(&self, mut next: ProjectData, strict: bool) -> Result<Prepared> {
        next.revision = self
            .data
            .revision
            .checked_add(1)
            .ok_or("Revision limit reached")?;
        validate_data(&next)?;
        let paths = assets::resolve(&next, self.assets.as_deref())?;
        let values = faber_expr::resolve(&next.params)?;
        let old_values = faber_expr::resolve(&self.data.params)?;
        let mut bodies = Vec::with_capacity(next.parts.len());
        let mut histories = self.histories.clone();
        histories.retain(|id, _| next.parts.contains_key(id));
        for (id, part) in &next.parts {
            let resolved = resolve_part(part, &values)?;
            check_part(id, &resolved)?;
            let previous = self
                .data
                .parts
                .get(id)
                .map(|p| resolve_part(p, &old_values))
                .transpose()?;
            if let Some(previous) = previous
                && previous.geometry == resolved.geometry
                && previous.placement == resolved.placement
                && let Some(body) = self.bodies.iter().find(|b| &b.id == id)
            {
                // Only the metadata changed. Keep the solid and its cache.
                bodies.push(Body {
                    name: resolved.name.clone(),
                    group: resolved.group.clone(),
                    colour: resolved.colour,
                    ..body.clone()
                });
                continue;
            }
            bodies.push(build_part(
                id,
                &resolved,
                self.kernel.as_ref(),
                histories.entry(id.clone()).or_default(),
                &paths,
                self.cache.as_deref(),
            )?);
        }
        if !next.sections.is_empty() {
            for (id, section) in &next.sections {
                let numbers = section.map_numbers(&mut |n| number(n, &values))?;
                section::world_plane(
                    self.kernel.as_ref(),
                    &next.parts,
                    &values,
                    &bodies,
                    &histories,
                    &numbers,
                )
                .map_err(|e| format!("Section {id}: {e}"))?;
            }
        }
        if strict {
            let checks = self
                .checks
                .as_ref()
                .ok_or("Design checks are not available")?;
            let previous = checks.errors(&self.data.contacts, &self.bodies)?;
            let current = checks.errors(&next.contacts, &bodies)?;
            for (id, message) in current {
                if !previous.contains_key(&id) {
                    return Err(format!("Strict change refused: {message}"));
                }
            }
        }
        Ok(Prepared {
            data: next,
            bodies,
            histories,
        })
    }
}

/// The key of a contact pair. Part ids cannot contain `|`.
pub fn contact_key(a: &str, b: &str) -> String {
    if a < b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

pub fn valid_id(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    !reserved
        && !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn validate_data(data: &ProjectData) -> Result<()> {
    if data.schema != 1 {
        return Err(format!("Unsupported schema: {}", data.schema));
    }
    if data.name.trim().is_empty() {
        return Err("Project name is empty".into());
    }
    if data.parts.len() > 2000 {
        return Err("Project has more than 2000 parts".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for id in data.parts.keys() {
        if !valid_id(id) || !names.insert(id.to_ascii_lowercase()) {
            return Err(format!(
                "Part id is reserved, invalid or differs only by case: {id}"
            ));
        }
    }
    for (key, contact) in &data.contacts {
        let known = |id: &str| data.parts.contains_key(id);
        if contact.a >= contact.b
            || key != &contact_key(&contact.a, &contact.b)
            || !known(&contact.a)
            || !known(&contact.b)
        {
            return Err(format!("Contact needs two known parts in order: {key}"));
        }
        if !contact.max_depth_mm.is_finite() || contact.max_depth_mm < 0. {
            return Err(format!(
                "Contact depth limit must be finite and not negative: {key}"
            ));
        }
    }
    for (id, section) in &data.sections {
        section::check_refs(data, id, section)?;
    }
    Ok(())
}

pub fn build(data: &ProjectData, kernel: &dyn Kernel) -> Result<Vec<Body>> {
    build_with_assets(data, kernel, None)
}

pub fn build_with_assets(
    data: &ProjectData,
    kernel: &dyn Kernel,
    assets: Option<&dyn faber_ports::AssetStore>,
) -> Result<Vec<Body>> {
    build_with_cache(data, kernel, assets, None)
}

/// Builds all parts. Built features also go to `cache` when the kernel has a cache version.
pub fn build_with_cache(
    data: &ProjectData,
    kernel: &dyn Kernel,
    assets: Option<&dyn faber_ports::AssetStore>,
    cache: Option<&dyn faber_ports::FeatureCache>,
) -> Result<Vec<Body>> {
    build_cached(data, kernel, &mut BTreeMap::new(), assets, cache)
}

fn build_cached(
    data: &ProjectData,
    kernel: &dyn Kernel,
    histories: &mut BTreeMap<String, history::Cache>,
    assets: Option<&dyn faber_ports::AssetStore>,
    cache: Option<&dyn faber_ports::FeatureCache>,
) -> Result<Vec<Body>> {
    validate_data(data)?;
    let paths = assets::resolve(data, assets)?;
    histories.retain(|id, _| data.parts.contains_key(id));
    let values = faber_expr::resolve(&data.params)?;
    data.parts
        .iter()
        .map(|(id, part)| {
            build_part(
                id,
                &resolve_part(part, &values)?,
                kernel,
                histories.entry(id.clone()).or_default(),
                &paths,
                cache,
            )
        })
        .collect()
}

fn resolve_part(part: &Part<Scalar>, values: &BTreeMap<String, f64>) -> Result<Part> {
    part.map_numbers(|value| number(value, values))
}

/// Resolves one number or expression with the parameter values.
pub(crate) fn number(value: &Scalar, values: &BTreeMap<String, f64>) -> Result<f64> {
    match value {
        Scalar::Number(value) if value.is_finite() => Ok(*value),
        Scalar::Number(_) => Err("Number is not finite".into()),
        Scalar::Expression(expression) => faber_expr::evaluate(expression, |name| {
            values
                .get(name)
                .copied()
                .ok_or_else(|| format!("Unknown parameter: {name}"))
        }),
    }
}

fn check_part(id: &str, part: &Part) -> Result<()> {
    if !valid_id(id) || part.name.trim().is_empty() {
        return Err("Part needs a valid id and a name".into());
    }
    Ok(())
}

/// Names the part before a shape failure. A history part also names its output feature.
fn part_error(id: &str, geometry: &faber_model::Geometry, error: String) -> String {
    match geometry {
        faber_model::Geometry::History { features, output } => {
            format!("{id}: {}", history::output_error(features, output, error))
        }
        _ => format!("{id}: {error}"),
    }
}

fn build_part(
    id: &str,
    part: &Part,
    kernel: &dyn Kernel,
    cache: &mut history::Cache,
    paths: &assets::Paths,
    persist: Option<&dyn faber_ports::FeatureCache>,
) -> Result<Body> {
    check_part(id, part)?;
    if part
        .placement
        .rotation
        .iter()
        .chain(&part.placement.translation)
        .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err(format!("{id}: placement is outside the supported range"));
    }
    let shape = match &part.geometry {
        faber_model::Geometry::History { features, output } => {
            history::build(features, output, kernel, cache, paths, persist).map(Some)
        }
        faber_model::Geometry::Imported { asset } => kernel
            .read_shape(paths.get(asset).ok_or("Asset is missing")?)
            .map(Some),
        geometry => kernel.exact_shape(geometry),
    }
    .map_err(|e| format!("{id}: {e}"))?;
    if let Some(shape) = shape {
        let placed = kernel
            .transform_shape(&shape, &part.placement)
            .map_err(|error| part_error(id, &part.geometry, error))?;
        let shape = provenance::track(kernel, placed, std::slice::from_ref(&shape), id)
            .map_err(|error| part_error(id, &part.geometry, error))?;
        let properties = kernel
            .properties(&shape)
            .map_err(|error| part_error(id, &part.geometry, error))?;
        if !properties.valid
            || properties.solids == 0
            || !properties.volume.is_finite()
            || properties.volume <= 0.
        {
            return Err(part_error(
                id,
                &part.geometry,
                "operation did not produce a valid solid".into(),
            ));
        }
        let mesh = kernel
            .tessellate(&shape, 0.15)
            .map_err(|error| part_error(id, &part.geometry, error))?;
        return Ok(Body {
            id: id.into(),
            name: part.name.clone(),
            group: part.group.clone(),
            colour: part.colour,
            mesh,
            volume: properties.volume,
            bounds: properties.bounds,
            shape: Some(shape),
        });
    }
    let mut mesh = kernel
        .build(&part.geometry)
        .map_err(|e| format!("{id}: {e}"))?;
    let [x, y, z] = part.placement.rotation.map(f64::to_radians);
    let rotation =
        DMat3::from_rotation_z(z) * DMat3::from_rotation_y(y) * DMat3::from_rotation_x(x);
    let translation = DVec3::from(part.placement.translation);
    for p in &mut mesh.vertices {
        *p = (rotation * DVec3::from(*p) + translation).to_array();
    }
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in &mesh.vertices {
        lo = lo.min(DVec3::from(*p));
        hi = hi.max(DVec3::from(*p));
    }
    let centre = (lo + hi) / 2.;
    let volume: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| DVec3::from(mesh.vertices[i as usize]) - centre);
            a.dot(b.cross(c)) / 6.
        })
        .sum();
    if !volume.is_finite() || volume <= 0. {
        return Err(format!("{id}: solid has no positive volume"));
    }
    Ok(Body {
        id: id.into(),
        name: part.name.clone(),
        group: part.group.clone(),
        colour: part.colour,
        mesh,
        volume,
        bounds: [lo.to_array(), hi.to_array()],
        shape: None,
    })
}
