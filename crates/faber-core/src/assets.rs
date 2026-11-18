use faber_model::{Geometry, ProjectData, Scalar};
use faber_ports::{AssetStore, Result};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) type Paths = BTreeMap<String, PathBuf>;

impl crate::Project {
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
    ) -> Result<()> {
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
                    visit(&feature.geometry, store, paths)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    for part in data.parts.values() {
        visit(&part.geometry, store, &mut paths)?;
    }
    Ok(paths)
}
