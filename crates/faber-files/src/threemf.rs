use faber_ports::{MeshAssemblyExport, MeshItem, Result};
use std::io::{BufWriter, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

pub struct ThreeMf;

const TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#;
const RELATIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/3dmodel.model"/></Relationships>"#;

fn validate(item: &MeshItem<'_>) -> Result<()> {
    if !item.name.chars().all(|c| matches!(c, '\t'|'\n'|'\r'|' '..='\u{d7ff}'|'\u{e000}'..='\u{fffd}'|'\u{10000}'..='\u{10ffff}')) {
        return Err("The part name contains an invalid XML character.".into())
    }
    let mesh = item.mesh;
    if mesh.triangles.is_empty()
        || mesh
            .vertices
            .iter()
            .flatten()
            .chain(&item.transform)
            .any(|v| !v.is_finite())
        || mesh
            .triangles
            .iter()
            .flatten()
            .any(|&index| index as usize >= mesh.vertices.len())
    {
        return Err(format!(
            "Part {} has an empty or invalid mesh or transform.",
            item.name
        ));
    }
    let axes: [f64; 9] = item.transform[..9].try_into().unwrap();
    let det = glam::DMat3::from_cols_array(&axes).determinant();
    if !det.is_finite() || det.abs() < 1e-15 {
        return Err(format!("Part {} has a singular transform.", item.name));
    }
    Ok(())
}

// A model needs two opposite uses of each indexed edge. Keep open input as a surface.
fn indexed(mesh: &faber_ports::Mesh) -> Result<(faber_ports::Mesh, &'static str)> {
    use glam::DVec3;
    use std::collections::HashMap;
    let (vertices, remap) = mesh.welded_vertices(1e-6)?;
    let origin = DVec3::from(*vertices.first().ok_or("The mesh has no vertices.")?);
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    let mut edges = HashMap::<(u32, u32), (usize, i64)>::new();
    let mut volume = 0.;
    for indices in &mesh.triangles {
        let face = indices.map(|i| remap[i as usize]);
        let [a, b, c] = face.map(|i| DVec3::from(vertices[i as usize]) - origin);
        let normal = (b - a).cross(c - a);
        if normal == DVec3::ZERO {
            continue;
        }
        volume += a.dot(normal);
        triangles.push(face);
        for (start, end) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
            let (count, orientation) = edges.entry((start.min(end), start.max(end))).or_default();
            *count += 1;
            *orientation += if start < end { 1 } else { -1 };
        }
    }
    if triangles.is_empty() || !volume.is_finite() {
        return Err("The mesh has no usable triangles or exceeds the numeric range.".into());
    }
    let solid = volume != 0. && edges.values().all(|&edge| edge == (2, 0));
    if solid && volume < 0. {
        for face in &mut triangles {
            face.swap(1, 2);
        }
    }
    Ok((
        faber_ports::Mesh {
            vertices,
            triangles,
            normals: Vec::new(),
        },
        if solid { "model" } else { "surface" },
    ))
}

fn model(out: &mut impl Write, items: &[MeshItem<'_>]) -> std::io::Result<usize> {
    write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?><model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" unit="millimeter" xml:lang="en"><resources><basematerials id="1">"#
    )?;
    for item in items {
        let [r, g, b] = item.colour;
        write!(
            out,
            r##"<base name="{}" displaycolor="#{r:02X}{g:02X}{b:02X}FF"/>"##,
            quick_xml::escape::escape(item.name)
        )?;
    }
    write!(out, "</basematerials>")?;
    let mut count = 0;
    for (index, item) in items.iter().enumerate() {
        let (mesh, kind) = indexed(item.mesh).map_err(std::io::Error::other)?;
        count += mesh.triangles.len();
        write!(
            out,
            r#"<object id="{}" type="{kind}" name="{}" pid="1" pindex="{index}"><mesh><vertices>"#,
            index + 2,
            quick_xml::escape::escape(item.name)
        )?;
        for [x, y, z] in &mesh.vertices {
            write!(out, r#"<vertex x="{x}" y="{y}" z="{z}"/>"#)?;
        }
        write!(out, "</vertices><triangles>")?;
        for [a, b, c] in &mesh.triangles {
            write!(out, r#"<triangle v1="{a}" v2="{b}" v3="{c}"/>"#)?;
        }
        write!(out, "</triangles></mesh></object>")?;
    }
    write!(out, "</resources><build>")?;
    for (index, item) in items.iter().enumerate() {
        write!(out, r#"<item objectid="{}" transform=""#, index + 2)?;
        for (position, v) in item.transform.iter().enumerate() {
            if position > 0 {
                write!(out, " ")?;
            }
            write!(out, "{v}")?;
        }
        write!(out, r#""/>"#)?;
    }
    write!(out, "</build></model>")?;
    Ok(count)
}

impl MeshAssemblyExport for ThreeMf {
    fn write_assembly(&self, items: &[MeshItem<'_>], path: &Path) -> Result<usize> {
        if items.is_empty() {
            return Err("Select at least one part to export.".into());
        }
        for item in items {
            validate(item)?;
        }
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file.as_file_mut());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in [("[Content_Types].xml", TYPES), ("_rels/.rels", RELATIONS)] {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        zip.start_file("3D/3dmodel.model", options)
            .map_err(|e| e.to_string())?;
        let count = {
            let mut out = BufWriter::new(&mut zip);
            let count = model(&mut out, items).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
            count
        };
        zip.finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(count)
    }
}
