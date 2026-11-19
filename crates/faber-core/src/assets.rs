use faber_model::{Geometry, ProjectData, Scalar};
use faber_ports::{AssetStore, Result};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) type Paths = BTreeMap<String, PathBuf>;
pub fn refuse_link_edit(id: &str, part: &faber_model::Part<Scalar>) -> Result<()> {
    if let Geometry::Imported {
        link: Some(link), ..
    } = &part.geometry
    {
        return Err(format!(
            "Part {id} follows the file {}. Change the file, then call refresh_links.",
            link.path
        ));
    }
    Ok(())
}

impl crate::Project {
    pub fn folder(&self) -> Option<&std::path::Path> {
        self.store.folder()
    }

    pub fn shape_result_id(&self, stock: &str) -> Result<String> {
        let owner = crate::instances::owner(&self.data, stock)?;
        if matches!(
            self.data.parts[owner].geometry,
            Geometry::Imported {
                purchased: Some(_),
                ..
            }
        ) {
            return Err("A purchased part cannot be optimised.".into());
        }
        let id = format!("{stock}-shape");
        if !crate::valid_id(&id) || self.data.parts.keys().any(|k| k.eq_ignore_ascii_case(&id)) {
            return Err(format!("The result needs a new valid part id: {id}."));
        }
        Ok(id)
    }

    /// Store one new result on the frame of its stock. The stock recipe stays intact.
    pub fn add_shape_result(
        &mut self,
        revision: u64,
        stock: &str,
        path: &std::path::Path,
        key: &str,
    ) -> Result<String> {
        self.check_edit_revision(revision)?;
        self.refuse_broken_source()?;
        let id = self.shape_result_id(stock)?;
        let store = self
            .assets
            .as_deref()
            .ok_or("Asset storage is not available")?;
        if !store.is_mesh(path)? || store.key(path)? != key {
            return Err("The shape result needs a mesh with its asset hash.".into());
        }
        let mut part = self.data.parts[stock].clone();
        part.name = format!("{} shape", part.name);
        part.material = crate::part_material(&self.data, stock).map(str::to_owned);
        part.geometry = Geometry::Imported {
            asset: key.to_owned(),
            purchased: None,
            link: None,
        };
        let mut candidate = self.data.clone();
        candidate.parts.insert(id.clone(), part);
        self.store.record_shape_result(stock, key)?;
        self.save_with_assets(candidate, [(key.to_owned(), path.to_owned())].into(), false)?;
        Ok(id)
    }

    /// Replace only the cached asset and hash of the listed file links.
    pub fn refresh_link_assets(
        &mut self,
        revision: u64,
        updates: BTreeMap<String, (String, PathBuf)>,
    ) -> Result<()> {
        self.check_edit_revision(revision)?;
        self.refuse_generated()?;
        let mut next = self.data.clone();
        let mut staged = Paths::new();
        for (id, (hash, path)) in updates {
            let part = next
                .parts
                .get_mut(&id)
                .ok_or_else(|| format!("Unknown linked part: {id}"))?;
            let Geometry::Imported {
                asset,
                link: Some(link),
                purchased: None,
            } = &mut part.geometry
            else {
                return Err(format!("Part {id} has no file link."));
            };
            asset.clone_from(&hash);
            link.hash.clone_from(&hash);
            staged.insert(hash, path);
        }
        self.save_with_assets(next, staged, false)
    }
    /// Read the imported triangle surface before the part placement.
    pub fn imported_mesh(&self, id: &str) -> Result<&faber_ports::Mesh> {
        let local = self
            .locals
            .get(id)
            .ok_or_else(|| format!("Unknown part: {id}"))?;
        if local.shape.is_some() || !matches!(local.geometry, Geometry::Imported { .. }) {
            return Err(format!("Part {id} must be an imported mesh."));
        }
        Ok(&local.mesh)
    }

    /// Identify a temporary source file before its part recipe is built.
    pub fn asset_key(&self, source: &std::path::Path) -> Result<String> {
        self.assets
            .as_deref()
            .ok_or("Asset storage is not available")?
            .key(source)
    }
}

pub(crate) fn mesh(
    geometry: &Geometry,
    store: Option<&dyn AssetStore>,
    paths: &Paths,
) -> Result<Option<faber_ports::Mesh>> {
    match (geometry, store) {
        (Geometry::Imported { asset, .. }, Some(store)) => {
            store.read_mesh(paths.get(asset).ok_or("Asset is missing")?)
        }
        _ => Ok(None),
    }
}

pub(crate) fn resolve(data: &ProjectData, store: Option<&dyn AssetStore>) -> Result<Paths> {
    resolve_with(data, store, Paths::new())
}

pub(crate) fn resolve_with(
    data: &ProjectData,
    store: Option<&dyn AssetStore>,
    mut paths: Paths,
) -> Result<Paths> {
    fn visit(
        geometry: &Geometry<Scalar>,
        store: Option<&dyn AssetStore>,
        paths: &mut Paths,
        root: bool,
    ) -> Result<()> {
        if let Geometry::Imported {
            asset,
            link: Some(link),
            purchased,
        } = geometry
        {
            if !root {
                return Err("A file link must be the root geometry of a part.".into());
            }
            if purchased.is_some()
                || link.path.trim().is_empty()
                || link.path.contains('\0')
                || link.hash != *asset
            {
                return Err("A file link needs a source path and its asset hash. A purchased part cannot have a file link.".into());
            }
        }
        match geometry {
            Geometry::Imported { asset, .. } if !paths.contains_key(asset) => {
                paths.insert(
                    asset.clone(),
                    store
                        .ok_or("Asset storage is not available")?
                        .resolve(asset)?,
                );
            }
            Geometry::History { features, .. } => {
                for feature in features {
                    visit(&feature.geometry, store, paths, false)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut checked = std::collections::BTreeSet::new();
    for (id, part) in &data.parts {
        visit(&part.geometry, store, &mut paths, true)?;
        let Geometry::History { features, .. } = &part.geometry else {
            continue;
        };
        if !features.iter().any(|f| {
            !f.suppressed
                && matches!(
                    f.geometry,
                    Geometry::RemoveFaces { .. }
                        | Geometry::MoveFaces { .. }
                        | Geometry::SplitFace { .. }
                        | Geometry::Emboss { .. }
                )
        }) {
            continue;
        }
        crate::pick_selector::editable_part(id, part)?;
        for f in features.iter().filter(|f| !f.suppressed) {
            if let Geometry::Imported { asset, .. } = &f.geometry
                && checked.insert(asset)
                && store
                    .ok_or("Asset storage is not available")?
                    .is_mesh(&paths[asset])?
            {
                return Err(format!("Part {id} is a mesh. Convert it to a solid first."));
            }
        }
    }
    Ok(paths)
}
