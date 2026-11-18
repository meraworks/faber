use faber_model::{Geometry, ProjectData, Scalar};
use faber_ports::{AssetStore, Result};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) type Paths = BTreeMap<String, PathBuf>;

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
    let mut paths = Paths::new();
    for part in data.parts.values() {
        visit(&part.geometry, store, &mut paths)?;
    }
    Ok(paths)
}
