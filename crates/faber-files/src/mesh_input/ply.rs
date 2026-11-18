use faber_ports::{Mesh, Result};

struct Property {
    name: String,
    item: String,
    count: Option<String>,
}
struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

pub(super) fn read(bytes: &[u8]) -> Result<Mesh> {
    let mut offset = 0;
    let mut format = None;
    let mut elements: Vec<Element> = Vec::new();
    loop {
        let end = bytes[offset..]
            .iter()
            .position(|&b| b == b'\n')
            .ok_or("PLY header is incomplete")?
            + offset;
        let line = std::str::from_utf8(&bytes[offset..end])
            .map_err(|_| "Invalid PLY header")?
            .trim();
        offset = end + 1;
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["ply"] | ["comment", ..] | ["obj_info", ..] => (),
            ["format", encoding, "1.0"] if format.is_none() => format = Some(*encoding),
            ["element", name, count] => {
                let count = count.parse().map_err(|_| "Invalid PLY element count")?;
                if count > bytes.len() {
                    return Err("PLY count exceeds input size".into());
                }
                elements.push(Element {
                    name: (*name).into(),
                    count,
                    properties: Vec::new(),
                });
            }
            ["property", item, name] => elements
                .last_mut()
                .ok_or("PLY property needs an element")?
                .properties
                .push(Property {
                    name: (*name).into(),
                    item: (*item).into(),
                    count: None,
                }),
            ["property", "list", count, item, name] => elements
                .last_mut()
                .ok_or("PLY property needs an element")?
                .properties
                .push(Property {
                    name: (*name).into(),
                    item: (*item).into(),
                    count: Some((*count).into()),
                }),
            ["end_header"] => break,
            _ => return Err("Unsupported PLY header field".into()),
        }
    }
    let format = format.ok_or("PLY encoding is missing")?;
    if !["ascii", "binary_little_endian", "binary_big_endian"].contains(&format) {
        return Err("Unsupported PLY encoding".into());
    }
    let mut ascii = if format == "ascii" {
        std::str::from_utf8(&bytes[offset..]).map_err(|_| "Invalid PLY text")?
    } else {
        ""
    }
    .split_whitespace();
    let mut value = |kind: &str| -> Result<f64> {
        let (size, signed, float) = match kind {
            "char" | "int8" => (1, true, false),
            "uchar" | "uint8" => (1, false, false),
            "short" | "int16" => (2, true, false),
            "ushort" | "uint16" => (2, false, false),
            "int" | "int32" => (4, true, false),
            "uint" | "uint32" => (4, false, false),
            "float" | "float32" => (4, true, true),
            "double" | "float64" => (8, true, true),
            _ => return Err("Unsupported PLY property type".into()),
        };
        if format == "ascii" {
            let v = super::number(ascii.next().ok_or("PLY data is incomplete")?)?;
            if !float {
                let bits = size * 8;
                let lo = if signed { -(2f64.powi(bits - 1)) } else { 0. };
                let hi = if signed {
                    2f64.powi(bits - 1) - 1.
                } else {
                    2f64.powi(bits) - 1.
                };
                if v.fract() != 0. || v < lo || v > hi {
                    return Err("PLY integer is out of range".into());
                }
            }
            return Ok(v);
        }
        let size = size as usize;
        let data = bytes
            .get(offset..offset + size)
            .ok_or("PLY data is incomplete")?;
        offset += size;
        let mut raw = [0u8; 8];
        if format == "binary_big_endian" {
            for (to, from) in raw.iter_mut().zip(data.iter().rev()) {
                *to = *from;
            }
        } else {
            raw[..size].copy_from_slice(data);
        }
        let bits = u64::from_le_bytes(raw);
        Ok(if float {
            if size == 4 {
                f32::from_bits(bits as u32) as f64
            } else {
                f64::from_bits(bits)
            }
        } else if signed {
            ((bits << (64 - size * 8)) as i64 >> (64 - size * 8)) as f64
        } else {
            bits as f64
        })
    };
    let mut mesh = Mesh::default();
    let mut faces = Vec::new();
    for element in elements {
        for _ in 0..element.count {
            let mut point = [None; 3];
            let mut face = None;
            for property in &element.properties {
                if let Some(count) = &property.count {
                    let count = value(count)?;
                    if count < 0. || count.fract() != 0. || count > bytes.len() as f64 {
                        return Err("Invalid PLY list size".into());
                    }
                    let is_face = element.name == "face"
                        && ["vertex_indices", "vertex_index"].contains(&property.name.as_str());
                    let mut indices = Vec::new();
                    for _ in 0..count as usize {
                        let v = value(&property.item)?;
                        if is_face {
                            if v < 0. || v > u32::MAX as f64 || v.fract() != 0. {
                                return Err("Invalid PLY vertex index".into());
                            }
                            indices.push(v as u32);
                        }
                    }
                    if is_face {
                        face = Some(indices);
                    }
                } else {
                    let v = value(&property.item)?;
                    if element.name == "vertex"
                        && let Some(axis) = ["x", "y", "z"].iter().position(|&n| n == property.name)
                    {
                        point[axis] = Some(v);
                    }
                }
            }
            if element.name == "vertex" {
                mesh.vertices.push([
                    point[0].ok_or("PLY x is missing")?,
                    point[1].ok_or("PLY y is missing")?,
                    point[2].ok_or("PLY z is missing")?,
                ]);
            }
            if element.name == "face" {
                faces.push(face.ok_or("PLY face indices are missing")?);
            }
        }
    }
    if (format == "ascii" && ascii.next().is_some()) || (format != "ascii" && offset != bytes.len())
    {
        return Err("Unexpected data after PLY elements".into());
    }
    for indices in faces {
        super::polygon::face(&mut mesh, &indices)?;
    }
    Ok(mesh)
}
