use faber_ports::{Mesh, Result};
use std::path::Path;
mod ply;
mod polygon;
mod three_mf;

const MAX_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn read(path: &Path) -> Result<Option<Mesh>> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("Model exceeds the 512 MiB input limit".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.trim_ascii_start().starts_with(b"ISO-10303-21;") {
        return Ok(None);
    }
    let text = std::str::from_utf8(&bytes).ok();
    let first = text.and_then(|s| polygon::lines(s).next()).unwrap_or("");
    let binary_stl = bytes.len() >= 84
        && 84u64 + 50 * u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as u64
            == bytes.len() as u64;
    let mesh = if binary_stl {
        stl(&bytes)?
    } else if bytes.starts_with(b"PK\x03\x04") {
        three_mf::read(&bytes)?
    } else if bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n") {
        ply::read(&bytes)?
    } else if first == "OFF" || first.starts_with("OFF ") {
        polygon::off(text.unwrap())?
    } else if text
        .is_some_and(|s| polygon::lines(s).any(|l| l.starts_with("v ") || l.starts_with("v\t")))
    {
        polygon::obj(text.unwrap())?
    } else {
        stl(&bytes)?
    };
    validate(&mesh)?;
    Ok(Some(mesh))
}

fn validate(mesh: &Mesh) -> Result<()> {
    if mesh.vertices.is_empty() || mesh.triangles.is_empty() {
        return Err("Model has no triangle faces".into());
    }
    if mesh.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Model coordinates must be finite".into());
    }
    for face in &mesh.triangles {
        if face.iter().any(|&i| i as usize >= mesh.vertices.len()) {
            return Err("Face vertex index is out of range".into());
        }
        let [a, b, c] = face.map(|i| glam::DVec3::from(mesh.vertices[i as usize]));
        if !(b - a).cross(c - a).length_squared().is_finite() {
            return Err("Model face exceeds the supported coordinate range".into());
        }
    }
    Ok(())
}

fn stl(bytes: &[u8]) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if 84usize.checked_add(count.saturating_mul(50)) == Some(bytes.len()) {
            for record in bytes[84..].as_chunks::<50>().0 {
                let start = mesh.vertices.len() as u32;
                for point in record[12..48].as_chunks::<12>().0 {
                    mesh.vertices.push(std::array::from_fn(|axis| {
                        f32::from_le_bytes(point[axis * 4..axis * 4 + 4].try_into().unwrap()) as f64
                    }));
                }
                mesh.triangles.push([start, start + 1, start + 2]);
            }
            return Ok(mesh);
        }
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "Invalid binary STL or unsupported model format")?;
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if !lines
        .next()
        .is_some_and(|line| line == "solid" || line.starts_with("solid "))
    {
        return Err("Unsupported model format or malformed STL".into());
    }
    loop {
        let line = lines.next().ok_or("STL has no endsolid")?;
        if line == "endsolid" || line.starts_with("endsolid ") {
            if lines.next().is_some() {
                return Err("Unexpected data after STL solid".into());
            }
            break;
        }
        let normal: Vec<_> = line.split_whitespace().collect();
        if normal.len() != 5 || normal[..2] != ["facet", "normal"] {
            return Err("STL needs a facet normal".into());
        }
        for value in &normal[2..] {
            number(value)?;
        }
        if lines.next() != Some("outer loop") {
            return Err("STL needs an outer loop".into());
        }
        let start = mesh.vertices.len() as u32;
        for _ in 0..3 {
            let values: Vec<_> = lines
                .next()
                .ok_or("STL vertex is missing")?
                .split_whitespace()
                .collect();
            if values.len() != 4 || values[0] != "vertex" {
                return Err("Invalid STL vertex".into());
            }
            mesh.vertices
                .push([number(values[1])?, number(values[2])?, number(values[3])?]);
        }
        if lines.next() != Some("endloop") || lines.next() != Some("endfacet") {
            return Err("STL facet is not closed".into());
        }
        mesh.triangles.push([start, start + 1, start + 2]);
    }
    Ok(mesh)
}

pub(super) fn number(text: &str) -> Result<f64> {
    let value: f64 = text.parse().map_err(|_| "Invalid model number")?;
    if !value.is_finite() {
        return Err("Model coordinates must be finite".into());
    }
    Ok(value)
}
