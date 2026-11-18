//! Project operations through ports.

mod assets;
mod batch;
mod body_cache;
mod construction;
mod dress_variant;
mod feature_cache;
mod feature_edit;
mod feature_info;
mod generated;
mod history;
mod hole;
pub mod materials;
pub mod names;
pub mod overhang;
mod pattern;
pub mod pick_selector;
mod pipe;
mod placement;
pub mod press_pull;
mod provenance;
mod read_cache;
mod scale;
mod section;
mod select;
pub mod study;
mod thread;

pub use batch::{Operation, PartInfo, PartInfoOperation};
pub use body_cache::face_triangles;
use faber_model::{Part, ProjectData, Scalar};
use faber_ports::{Body, Kernel, ProjectStore, Result, Shape};
pub use feature_info::{FeatureInfo, Reading};
pub use generated::regenerate;
use glam::DVec3;
pub use placement::{
    AlignMode, Axis, Motion, Pivot, PivotName, PointToPoint, Rotation, RotationAxis, align_shift,
    euler_from_matrix, move_edits, refuse_purchased_copy, resolved as resolved_placements,
    rotation_matrix, tidy, union_box,
};
pub use section::{CutPart, ResolvedSection, SectionReport};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Mass properties of a placed part in world coordinates.
pub struct MassReport {
    pub material: Option<String>,
    /// The density used for mass and inertia.
    pub density_g_cm3: Option<f64>,
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub centre_of_mass_mm: [f64; 3],
    /// Present when the part or the request supplies a density.
    pub mass_g: Option<f64>,
    /// Present when the part or the request supplies a density.
    pub inertia_kg_mm2: Option<[[f64; 3]; 3]>,
}

/// Edge and face lists. A list that the kind leaves out is None.
pub type TopologyLists = (
    Option<Vec<faber_ports::EdgeInfo>>,
    Option<Vec<faber_ports::FaceInfo>>,
);

/// Which lists a topology read returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyKind {
    Both,
    Edges,
    Faces,
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
    /// The design source that generates this project, if any.
    source: Option<String>,
    /// The error of the last design source build, if it failed.
    source_error: Option<String>,
    locals: BTreeMap<String, body_cache::LocalBody>,
    /// Results of read calls for the current revision.
    read_cache: Mutex<read_cache::ReadCache>,
}

struct Prepared {
    data: ProjectData,
    bodies: Vec<Body>,
    histories: BTreeMap<String, history::Cache>,
    locals: BTreeMap<String, body_cache::LocalBody>,
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
        let mut locals = BTreeMap::new();
        let bodies = build_cached(
            &data,
            kernel.as_ref(),
            &mut histories,
            &mut locals,
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
            source: None,
            source_error: None,
            locals,
            read_cache: Mutex::default(),
        })
    }

    pub fn sync(&mut self) -> Result<()> {
        let data = self.store.load()?;
        assets::resolve(&data, self.assets.as_deref())?;
        if data != self.data {
            let mut histories = self.histories.clone();
            let mut locals = self.locals.clone();
            let bodies = build_cached(
                &data,
                self.kernel.as_ref(),
                &mut histories,
                &mut locals,
                self.assets.as_deref(),
                self.cache.as_deref(),
            )?;
            self.data = data;
            self.bodies = bodies;
            self.histories = histories;
            self.locals = locals;
            self.clear_reads();
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
        self.import_with_record(id, name, source, strict, None)
    }

    pub fn import_with_record(
        &mut self,
        id: String,
        name: String,
        source: &std::path::Path,
        strict: bool,
        purchased: Option<faber_model::Purchased>,
    ) -> Result<()> {
        self.refuse_generated()?;
        if let Some(record) = &purchased
            && (record.source.trim().is_empty()
                || record.licence.trim().is_empty()
                || record.designation.trim().is_empty()
                || record.mass_g.is_some_and(|m| !m.is_finite() || m <= 0.))
        {
            return Err("Supply a source, licence and designation. Mass must be positive.".into());
        }
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
            material: None,
            name,
            group: String::new(),
            note: String::new(),
            colour: [100, 140, 170],
            placement: Default::default(),
            geometry: faber_model::Geometry::<Scalar>::Imported {
                asset,
                purchased: purchased.map(Box::new),
            },
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
            let part = part.map_numbers(|n| Ok(n.clone().into()))?;
            self.refuse_generated_part(&id, &part)?;
            if let Some(previous) = next.parts.get(&id)
                && matches!(
                    &previous.geometry,
                    faber_model::Geometry::Imported {
                        purchased: Some(_),
                        ..
                    }
                )
                && previous.geometry != part.geometry
            {
                return Err(format!("Purchased part geometry is fixed: {id}"));
            }
            next.parts.insert(id, part);
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
        self.refuse_generated()?;
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
        let material = self
            .data
            .parts
            .get(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?
            .material
            .clone();
        let density_g_cm3 = match (density_g_cm3, material.as_deref()) {
            (None, Some(id)) => Some(materials::get(&self.data, id)?.density_g_cm3),
            (density, _) => density,
        };
        let properties = self.cached("mass_properties", id, || {
            let shape = self
                .bodies
                .iter()
                .find(|body| body.id == id)
                .ok_or_else(|| format!("Unknown part: {id}"))?
                .shape
                .as_ref()
                .ok_or_else(|| format!("Part {id} has no exact shape"))?;
            self.kernel.mass_properties(shape)
        })?;
        let mass = if let Some(density) = density_g_cm3 {
            let grams = (properties.volume_mm3 * 1e-3) * density;
            let inertia = properties
                .inertia_unit_mm5
                .map(|row| row.map(|v| (v * 1e-6) * density));
            if !grams.is_finite() || inertia.iter().flatten().any(|v| !v.is_finite()) {
                return Err("The mass or inertia exceeds the number range.".into());
            }
            Some((grams, inertia))
        } else {
            None
        };
        Ok(MassReport {
            material,
            density_g_cm3,
            volume_mm3: properties.volume_mm3,
            area_mm2: properties.area_mm2,
            centre_of_mass_mm: properties.centre_of_mass_mm,
            mass_g: mass.map(|(grams, _)| grams),
            inertia_kg_mm2: mass.map(|(_, inertia)| inertia),
        })
    }

    /// Runs a read once for each revision. The result is kept in the read cache.
    pub fn cached<T: Clone + Send + 'static>(
        &self,
        tool: &'static str,
        key: &str,
        make: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let revision = self.data.revision;
        let kept = self
            .read_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get::<T>(revision, tool, key);
        if let Some(value) = kept {
            return Ok(value);
        }
        let value = make()?;
        self.read_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .put(revision, tool, key.to_string(), value.clone());
        Ok(value)
    }

    fn clear_reads(&mut self) {
        self.read_cache
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
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
    /// A selector count is not checked here. A list that `kind` leaves out is None.
    pub fn selected_topology_kind(
        &self,
        id: &str,
        feature: Option<&str>,
        kind: TopologyKind,
        edge: Option<&faber_model::EdgeSelector>,
        face: Option<&faber_model::FaceSelector>,
    ) -> Result<TopologyLists> {
        let key = format!("{id:?}/{feature:?}");
        let shape = self.topology_shape(id, feature)?;
        let kernel = self.kernel.as_ref();
        let edges = match kind {
            TopologyKind::Faces => None,
            _ => {
                let list = self.cached("edges", &key, || kernel.edges(shape))?;
                Some(match edge {
                    Some(selector) => select::filter_edges(kernel, shape, list, selector)?,
                    None => list,
                })
            }
        };
        let faces = match kind {
            TopologyKind::Edges => None,
            _ => {
                let list = self.cached("faces", &key, || kernel.faces(shape))?;
                Some(match face {
                    Some(selector) => select::filter_faces(shape, list, selector)?,
                    None => list,
                })
            }
        };
        Ok((edges, faces))
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
        let (edges, faces) =
            self.selected_topology_kind(id, feature, TopologyKind::Both, edge, face)?;
        Ok((edges.unwrap_or_default(), faces.unwrap_or_default()))
    }

    fn topology_shape(&self, id: &str, feature: Option<&str>) -> Result<&Shape> {
        match feature {
            Some(feature) => self.feature_shape(id, feature),
            None => {
                let body = self
                    .bodies
                    .iter()
                    .find(|body| body.id == id)
                    .ok_or_else(|| format!("Unknown part: {id}"))?;
                body.shape
                    .as_ref()
                    .ok_or_else(|| "Part has no exact shape".to_string())
            }
        }
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
        self.refuse_generated()?;
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
        self.refuse_plain_history()?;
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
        self.refuse_plain_history()?;
        self.sync()?;
        if !valid_id(name) {
            return Err("Invalid checkpoint name".into());
        }
        let target = self.store.read_checkpoint(name)?;
        self.apply(target)
    }
    pub fn redo(&mut self) -> Result<()> {
        self.refuse_plain_history()?;
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
        self.refuse_broken_source()?;
        // A change that only sets the revision is a no-op. It saves nothing and adds no undo step.
        // Undo and redo always move the history cursor, so they never take this exit.
        let unchanged = ProjectData {
            revision: self.data.revision,
            ..next.clone()
        };
        if action.is_none() && unchanged == self.data {
            return Ok(());
        }
        let Prepared {
            data: next,
            bodies,
            histories,
            locals,
        } = self.prepare(next, strict)?;
        match action {
            Some(action) => self.store.save_history(&self.data, &next, action)?,
            None => self.store.save_checked(&self.data, &next)?,
        }
        self.data = next;
        self.bodies = bodies;
        self.histories = histories;
        self.locals = locals;
        self.clear_reads();
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
        let mut locals = self.locals.clone();
        locals.retain(|id, _| next.parts.contains_key(id));
        let builder = Builder {
            kernel: self.kernel.as_ref(),
            paths: &paths,
            persist: self.cache.as_deref(),
        };
        // Parts whose placement or geometry changed, and those whose geometry changed.
        let mut changed = std::collections::BTreeSet::new();
        let mut geometry_changed = std::collections::BTreeSet::new();
        let mut timings = Vec::new();
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
            if !locals
                .get(id)
                .is_some_and(|local| local.geometry == resolved.geometry)
            {
                geometry_changed.insert(id.clone());
            }
            changed.insert(id.clone());
            let (body, timing) = builder.part(
                id,
                &resolved,
                histories.entry(id.clone()).or_default(),
                || assets::mesh(&resolved.geometry, self.assets.as_deref(), &paths),
                &mut locals,
            )?;
            timings.push((id.clone(), timing));
            bodies.push(body);
        }
        let sections_started = std::time::Instant::now();
        for (id, section) in &next.sections {
            let numbers = section.map_numbers(&mut |n| number(n, &values))?;
            if self.section_kept(
                id,
                section,
                &numbers,
                &old_values,
                &changed,
                &geometry_changed,
            )? {
                continue;
            }
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
        body_cache::report_sections(body_cache::elapsed(sections_started));
        for (id, timing) in &timings {
            body_cache::report(id, timing);
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
            locals,
        })
    }

    /// True when the stored section result still holds. The section must be unchanged, and the
    /// parts it reads must keep their geometry. A face selection without a feature also reads
    /// the placed shape, so its part must keep its placement too. Its resolved numbers must be equal.
    fn section_kept(
        &self,
        id: &str,
        section: &faber_model::SectionAnalysis<Scalar>,
        numbers: &faber_model::SectionAnalysis<f64>,
        old_values: &BTreeMap<String, f64>,
        changed: &std::collections::BTreeSet<String>,
        geometry_changed: &std::collections::BTreeSet<String>,
    ) -> Result<bool> {
        let Some(old) = self.data.sections.get(id) else {
            return Ok(false);
        };
        let placed_selection = matches!(
            &section.plane,
            faber_model::SectionPlane::Face { feature: None, .. }
        );
        let reads_changed_part = section_part(&section.plane).is_some_and(|part| {
            geometry_changed.contains(part) || (placed_selection && changed.contains(part))
        });
        if old != section || reads_changed_part {
            return Ok(false);
        }
        Ok(old.map_numbers(&mut |n| number(n, old_values))? == *numbers)
    }
}

/// The part that a section plane reads, if any.
fn section_part(plane: &faber_model::SectionPlane<Scalar>) -> Option<&str> {
    match plane {
        faber_model::SectionPlane::Plane { .. } => None,
        faber_model::SectionPlane::Reference { part, .. }
        | faber_model::SectionPlane::Face { part, .. } => Some(part),
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
    faber_ports::valid_id(id)
}

fn validate_data(data: &ProjectData) -> Result<()> {
    if data.schema != 1 {
        return Err(format!("Unsupported schema: {}", data.schema));
    }
    if data.name.trim().is_empty() {
        return Err("Project name is empty".into());
    }
    materials::validate(data)?;
    if !data.studies.is_empty() {
        let values = faber_expr::resolve(&data.params)?;
        for (id, study) in &data.studies {
            if !valid_id(id) {
                return Err(format!("Invalid study id: {id}"));
            }
            study::resolve_study(study, &values)?;
            if let Some(material) = &study.material {
                materials::get(data, material)?;
            }
            if !data.parts.contains_key(&study.part) {
                return Err(format!(
                    "Study {id} refers to an unknown part: {}",
                    study.part
                ));
            }
        }
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
    for id in data.motion.solve(&BTreeMap::new())?.keys() {
        if !data.parts.contains_key(id) {
            return Err(format!("Unknown motion part: {id}"));
        }
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

/// The unplaced body of one part in a candidate state. A batch placement reads its box from it.
/// It writes no feature cache.
pub(crate) fn candidate_local(
    data: &ProjectData,
    id: &str,
    kernel: &dyn Kernel,
    assets: Option<&dyn faber_ports::AssetStore>,
) -> Result<body_cache::LocalBody> {
    let paths = assets::resolve(data, assets)?;
    let values = faber_expr::resolve(&data.params)?;
    let part = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?;
    let resolved = resolve_part(part, &values)?;
    let builder = Builder {
        kernel,
        paths: &paths,
        persist: None,
    };
    let mut timing = body_cache::Timing::default();
    builder.local(
        id,
        &resolved,
        &mut history::Cache::default(),
        || assets::mesh(&resolved.geometry, assets, &paths),
        &mut timing,
    )
}

/// Builds all parts. Built features also go to `cache` when the kernel has a cache version.
pub fn build_with_cache(
    data: &ProjectData,
    kernel: &dyn Kernel,
    assets: Option<&dyn faber_ports::AssetStore>,
    cache: Option<&dyn faber_ports::FeatureCache>,
) -> Result<Vec<Body>> {
    build_cached(
        data,
        kernel,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
        assets,
        cache,
    )
}

fn build_cached(
    data: &ProjectData,
    kernel: &dyn Kernel,
    histories: &mut BTreeMap<String, history::Cache>,
    locals: &mut BTreeMap<String, body_cache::LocalBody>,
    assets: Option<&dyn faber_ports::AssetStore>,
    cache: Option<&dyn faber_ports::FeatureCache>,
) -> Result<Vec<Body>> {
    validate_data(data)?;
    let paths = assets::resolve(data, assets)?;
    histories.retain(|id, _| data.parts.contains_key(id));
    locals.retain(|id, _| data.parts.contains_key(id));
    let values = faber_expr::resolve(&data.params)?;
    let builder = Builder {
        kernel,
        paths: &paths,
        persist: cache,
    };
    data.parts
        .iter()
        .map(|(id, part)| {
            let resolved = resolve_part(part, &values)?;
            let (body, timing) = builder.part(
                id,
                &resolved,
                histories.entry(id.clone()).or_default(),
                || assets::mesh(&resolved.geometry, assets, &paths),
                locals,
            )?;
            body_cache::report(id, &timing);
            Ok(body)
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

/// Builds parts with the kernel, the asset paths and the feature cache.
struct Builder<'a> {
    kernel: &'a dyn Kernel,
    paths: &'a assets::Paths,
    persist: Option<&'a dyn faber_ports::FeatureCache>,
}

/// Tessellation tolerance of the unplaced mesh, in mm.
const TESSELLATION_MM: f64 = 0.03;

impl Builder<'_> {
    /// Builds one part. A local body with equal geometry is placed again, not built.
    fn part(
        &self,
        id: &str,
        part: &Part,
        cache: &mut history::Cache,
        imported: impl FnOnce() -> Result<Option<faber_ports::Mesh>>,
        locals: &mut BTreeMap<String, body_cache::LocalBody>,
    ) -> Result<(Body, body_cache::Timing)> {
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
        let mut timing = body_cache::Timing::default();
        if !locals
            .get(id)
            .is_some_and(|local| local.geometry == part.geometry)
        {
            let local = self.local(id, part, cache, imported, &mut timing)?;
            locals.insert(id.into(), local);
        }
        let local = &locals[id];
        let placed = body_cache::place_timed(local, &part.placement, self.kernel, &mut timing)
            .map_err(|error| part_error(id, &part.geometry, error))?;
        let started = std::time::Instant::now();
        // A placement is rigid, so the placed faces keep the unplaced face origins.
        let shape = match (placed.shape, &local.shape) {
            (Some(shape), Some(source)) => Some(match source.face_origins() {
                Some(origins) => shape.with_face_origins(origins.clone()),
                None => shape,
            }),
            _ => None,
        };
        timing.track = body_cache::elapsed(started);
        let body = Body {
            id: id.into(),
            name: part.name.clone(),
            group: part.group.clone(),
            colour: part.colour,
            mesh: placed.mesh,
            face_of_triangle: local.face_of_triangle.clone(),
            edge_lines: placed.edge_lines,
            volume: local.volume,
            bounds: placed.bounds,
            shape,
        };
        Ok((body, timing))
    }

    /// Builds the unplaced body of one part.
    fn local(
        &self,
        id: &str,
        part: &Part,
        cache: &mut history::Cache,
        imported: impl FnOnce() -> Result<Option<faber_ports::Mesh>>,
        timing: &mut body_cache::Timing,
    ) -> Result<body_cache::LocalBody> {
        let imported = imported().map_err(|error| format!("{id}: {error}"))?;
        let is_imported = imported.is_some();
        let started = std::time::Instant::now();
        let (shape, output_key) = if is_imported {
            (None, None)
        } else {
            self.shape(part, cache)
                .map_err(|error| format!("{id}: {error}"))?
        };
        timing.history = body_cache::elapsed(started);
        if let Some(shape) = shape {
            let started = std::time::Instant::now();
            let properties = self
                .kernel
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
            timing.properties = body_cache::elapsed(started);
            let started = std::time::Instant::now();
            let (mesh, edge_lines) = self
                .tessellate(
                    &shape,
                    properties.faces,
                    properties.edges,
                    output_key.as_deref(),
                )
                .map_err(|error| part_error(id, &part.geometry, error))?;
            timing.tessellate = body_cache::elapsed(started);
            return Ok(body_cache::LocalBody {
                geometry: part.geometry.clone(),
                shape: Some(shape),
                face_of_triangle: mesh.face_of_triangle,
                edge_lines,
                mesh: mesh.mesh,
                volume: properties.volume,
                bounds: properties.bounds,
            });
        }
        let mesh = match imported {
            Some(mesh) => mesh,
            None => self
                .kernel
                .build(&part.geometry)
                .map_err(|e| format!("{id}: {e}"))?,
        };
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for p in mesh.triangle_vertices() {
            lo = lo.min(DVec3::from(p));
            hi = hi.max(DVec3::from(p));
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
        let volume = if is_imported { volume.abs() } else { volume };
        if !volume.is_finite() || (!is_imported && volume <= 0.) {
            return Err(format!("{id}: solid has no positive volume"));
        }
        Ok(body_cache::LocalBody {
            geometry: part.geometry.clone(),
            shape: None,
            mesh,
            face_of_triangle: Vec::new(),
            edge_lines: Default::default(),
            volume,
            bounds: [lo.to_array(), hi.to_array()],
        })
    }

    /// The exact shape of a history, asset or primitive part. A history also returns the
    /// feature cache key of its output, when that cache is active.
    fn shape(
        &self,
        part: &Part,
        cache: &mut history::Cache,
    ) -> Result<(Option<Shape>, Option<String>)> {
        Ok(match &part.geometry {
            faber_model::Geometry::History { features, output } => {
                let (shape, key) = history::build(
                    features,
                    output,
                    self.kernel,
                    cache,
                    self.paths,
                    self.persist,
                )?;
                (Some(shape), key)
            }
            faber_model::Geometry::Imported { asset, .. } => (
                Some(
                    self.kernel
                        .read_shape(self.paths.get(asset).ok_or("Asset is missing")?)?,
                ),
                None,
            ),
            geometry => (self.kernel.exact_shape(geometry)?, None),
        })
    }

    /// Tessellates an unplaced shape. A history output reads its mesh from the feature cache
    /// when the cache has it.
    fn tessellate(
        &self,
        shape: &Shape,
        face_count: u32,
        edge_count: u32,
        output_key: Option<&str>,
    ) -> Result<(faber_ports::FaceMesh, faber_ports::EdgeLines)> {
        let valid = |faces: &faber_ports::FaceMesh| {
            !faces.mesh.triangles.is_empty()
                && faces.face_of_triangle.len() == faces.mesh.triangles.len()
                && faces
                    .face_of_triangle
                    .iter()
                    .all(|i| (1..=face_count).contains(i))
        };
        // The prefix identifies the 0.03 mm mesh with faces and edge lines.
        let cached = match (self.persist, output_key) {
            (Some(store), Some(key)) => Some((store, format!("mesh05{key}"))),
            _ => None,
        };
        if let Some((store, name)) = &cached
            && let Some((mesh, lines)) = store
                .get(name)
                .as_deref()
                .and_then(body_cache::decode_body_mesh)
            && valid(&mesh)
            && body_cache::valid_edge_lines(&lines, edge_count)
        {
            return Ok((mesh, lines));
        }
        let lines = match self.kernel.tessellate_edges(shape, TESSELLATION_MM) {
            Ok(lines) if body_cache::valid_edge_lines(&lines, edge_count) => lines,
            Ok(_) => return Err("Edge lines do not match the topology".into()),
            Err(error) if error == faber_ports::EDGE_LINES_UNSUPPORTED => Default::default(),
            Err(error) => return Err(error),
        };
        let mesh = match self.kernel.tessellate_faces(shape, TESSELLATION_MM) {
            Err(error) if error == faber_ports::FACE_MESH_UNSUPPORTED => {
                return Ok((
                    faber_ports::FaceMesh {
                        mesh: self.kernel.tessellate(shape, TESSELLATION_MM)?,
                        face_of_triangle: Vec::new(),
                    },
                    lines,
                ));
            }
            result => result?,
        };
        if !valid(&mesh) {
            return Err("Each triangle needs a face index".into());
        }
        if let Some((store, name)) = &cached
            && (!lines.is_empty() || edge_count == 0)
        {
            // A failed write only means that the next open tessellates again.
            let _ = store.put(name, &body_cache::encode_body_mesh(&mesh, &lines));
        }
        Ok((mesh, lines))
    }
}
