use faber_ports::{Mesh, Result};
use glam::{DMat4, DVec3, DVec4};
use roxmltree::{Document, Node};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

const CORE: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const PRODUCTION: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";

struct Instance {
    path: String,
    id: u32,
    transform: DMat4,
}
enum Object {
    Mesh(Mesh),
    Components(Vec<Instance>),
}
struct Model {
    objects: BTreeMap<u32, Object>,
    items: Vec<Instance>,
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Result<&'a str> {
    node.attribute(name)
        .ok_or_else(|| format!("3MF attribute is missing: {name}"))
}
fn index(node: Node<'_, '_>, name: &str) -> Result<u32> {
    attr(node, name)?
        .parse()
        .map_err(|_| format!("Invalid 3MF index: {name}"))
}
fn path(base: &str, target: &str) -> Result<String> {
    if target.contains(['\\', ':', '%', '?', '#']) {
        return Err("Unsupported 3MF resource path".into());
    }
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_owned()
    } else {
        format!(
            "{}{target}",
            base.rsplit_once('/')
                .map(|(p, _)| format!("{p}/"))
                .unwrap_or_default()
        )
    };
    let mut parts = Vec::new();
    for p in combined.split('/') {
        match p {
            "" | "." => (),
            ".." => {
                if parts.pop().is_none() {
                    return Err("3MF resource is outside the package".into());
                }
            }
            _ => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}
fn instance(node: Node<'_, '_>, source: &str, unit: f64) -> Result<Instance> {
    let mut transform = DMat4::IDENTITY;
    if let Some(text) = node.attribute("transform") {
        let v = text
            .split_whitespace()
            .map(super::number)
            .collect::<Result<Vec<_>>>()?;
        if v.len() != 12 {
            return Err("3MF transform needs twelve values".into());
        }
        transform = DMat4::from_cols(
            DVec4::new(v[0], v[1], v[2], 0.),
            DVec4::new(v[3], v[4], v[5], 0.),
            DVec4::new(v[6], v[7], v[8], 0.),
            DVec4::new(v[9] * unit, v[10] * unit, v[11] * unit, 1.),
        );
        if transform.determinant().abs() < 1e-15 {
            return Err("3MF transform is singular".into());
        }
    }
    let target = node
        .attribute((PRODUCTION, "path"))
        .map(|p| path(source, p))
        .transpose()?
        .unwrap_or_else(|| source.into());
    Ok(Instance {
        path: target,
        id: index(node, "objectid")?,
        transform,
    })
}
fn model(text: &str, source: &str) -> Result<Model> {
    let doc = Document::parse(text).map_err(|e| format!("Invalid 3MF model: {e}"))?;
    let root = doc.root_element();
    if !root.has_tag_name((CORE, "model")) {
        return Err("Invalid 3MF model root".into());
    }
    for prefix in root
        .attribute("requiredextensions")
        .unwrap_or("")
        .split_whitespace()
    {
        if root.lookup_namespace_uri(Some(prefix)) != Some(PRODUCTION) {
            return Err(format!("Unsupported required 3MF extension: {prefix}"));
        }
    }
    let unit = match root.attribute("unit").unwrap_or("millimeter") {
        "micron" => 0.001,
        "millimeter" => 1.,
        "centimeter" => 10.,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.,
        _ => return Err("Unsupported 3MF unit".into()),
    };
    let resources = root
        .children()
        .find(|n| n.has_tag_name((CORE, "resources")))
        .ok_or("3MF resources are missing")?;
    let mut objects = BTreeMap::new();
    for node in resources
        .children()
        .filter(|n| n.has_tag_name((CORE, "object")))
    {
        let id = index(node, "id")?;
        if id == 0 || objects.contains_key(&id) {
            return Err("3MF object id must be positive and unique".into());
        }
        if !["model", "support", "solidsupport", "surface", "other"]
            .contains(&node.attribute("type").unwrap_or("model"))
        {
            return Err("Unsupported 3MF object type".into());
        }
        let mesh_node = node.children().find(|n| n.has_tag_name((CORE, "mesh")));
        let components = node
            .children()
            .find(|n| n.has_tag_name((CORE, "components")));
        let object = match (mesh_node, components) {
            (Some(mesh_node), None) => {
                let mut mesh = Mesh::default();
                let vertices = mesh_node
                    .children()
                    .find(|n| n.has_tag_name((CORE, "vertices")))
                    .ok_or("3MF vertices are missing")?;
                for v in vertices
                    .children()
                    .filter(|n| n.has_tag_name((CORE, "vertex")))
                {
                    mesh.vertices.push([
                        super::number(attr(v, "x")?)? * unit,
                        super::number(attr(v, "y")?)? * unit,
                        super::number(attr(v, "z")?)? * unit,
                    ]);
                }
                let triangles = mesh_node
                    .children()
                    .find(|n| n.has_tag_name((CORE, "triangles")))
                    .ok_or("3MF triangles are missing")?;
                for t in triangles
                    .children()
                    .filter(|n| n.has_tag_name((CORE, "triangle")))
                {
                    mesh.triangles
                        .push([index(t, "v1")?, index(t, "v2")?, index(t, "v3")?]);
                }
                super::validate(&mesh)?;
                Object::Mesh(mesh)
            }
            (None, Some(components)) => Object::Components(
                components
                    .children()
                    .filter(|n| n.has_tag_name((CORE, "component")))
                    .map(|n| instance(n, source, unit))
                    .collect::<Result<_>>()?,
            ),
            _ => return Err("3MF object needs one mesh or component list".into()),
        };
        objects.insert(id, object);
    }
    let items = root
        .children()
        .find(|n| n.has_tag_name((CORE, "build")))
        .map(|build| {
            build
                .children()
                .filter(|n| n.has_tag_name((CORE, "item")))
                .filter(|n| {
                    n.attribute("printable") != Some("0")
                        && n.attribute("printable") != Some("false")
                })
                .map(|n| instance(n, source, unit))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Model { objects, items })
}

fn append(
    instance: &Instance,
    parent: DMat4,
    models: &BTreeMap<String, Model>,
    stack: &mut Vec<(String, u32)>,
    output: &mut Mesh,
    visits: &mut usize,
) -> Result<()> {
    *visits += 1;
    if *visits > 100_000 {
        return Err("3MF component count exceeds the import limit".into());
    }
    let key = (instance.path.clone(), instance.id);
    if stack.len() >= 64 || stack.contains(&key) {
        return Err("3MF component cycle or depth limit".into());
    }
    let object = models
        .get(&instance.path)
        .and_then(|m| m.objects.get(&instance.id))
        .ok_or("3MF object reference is missing")?;
    let transform = parent * instance.transform;
    stack.push(key);
    match object {
        Object::Mesh(mesh) => {
            if output.vertices.len() + mesh.vertices.len() > 10_000_000
                || output.triangles.len() + mesh.triangles.len() > 20_000_000
            {
                return Err("3MF expanded geometry exceeds the import limit".into());
            }
            let start = output.vertices.len() as u32;
            output.vertices.extend(
                mesh.vertices
                    .iter()
                    .map(|p| transform.transform_point3(DVec3::from(*p)).to_array()),
            );
            output.triangles.extend(mesh.triangles.iter().map(|t| {
                let mut t = t.map(|i| i + start);
                if transform.determinant() < 0. {
                    t.swap(1, 2);
                }
                t
            }));
        }
        Object::Components(items) => {
            if items.is_empty() {
                return Err("3MF component list is empty".into());
            }
            for child in items {
                append(child, transform, models, stack, output, visits)?;
            }
        }
    }
    stack.pop();
    Ok(())
}

pub(super) fn read(bytes: &[u8]) -> Result<Mesh> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Invalid 3MF package: {e}"))?;
    let mut texts = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_owned();
        if name != "_rels/.rels" && !name.ends_with(".model") {
            continue;
        }
        total = total
            .checked_add(entry.size())
            .ok_or("3MF package is too large")?;
        if total > super::MAX_BYTES {
            return Err("3MF expanded input exceeds 512 MiB".into());
        }
        let mut text = String::new();
        entry
            .take(super::MAX_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() as u64 > super::MAX_BYTES {
            return Err("3MF entry exceeds 512 MiB".into());
        }
        if texts.insert(name, text).is_some() {
            return Err("3MF package has duplicate entries".into());
        }
    }
    let rels = texts
        .get("_rels/.rels")
        .ok_or("3MF package relationship is missing")?;
    let doc = Document::parse(rels).map_err(|e| e.to_string())?;
    let mut roots = doc.descendants().filter(|n| {
        n.is_element()
            && n.attribute("Type")
                == Some("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel")
    });
    let relationship = roots.next().ok_or("3MF start model is missing")?;
    if roots.next().is_some() || relationship.attribute("TargetMode") == Some("External") {
        return Err("3MF needs one internal start model".into());
    }
    let root = path("", attr(relationship, "Target")?)?;
    let models = texts
        .iter()
        .filter(|(name, _)| name.ends_with(".model"))
        .map(|(name, text)| Ok((name.clone(), model(text, name)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut mesh = Mesh::default();
    let mut visits = 0;
    for item in &models
        .get(&root)
        .ok_or("3MF start model was not found")?
        .items
    {
        append(
            item,
            DMat4::IDENTITY,
            &models,
            &mut Vec::new(),
            &mut mesh,
            &mut visits,
        )?;
    }
    Ok(mesh)
}
