//! Keys and entries of the persistent feature cache. The shape bytes stay opaque.

use faber_ports::FaceOrigins;
use sha2::{Digest, Sha256};

/// Version of the entry layout, and the build of this crate. A new build can change results.
const LAYOUT: &str = concat!("faber-feature-1 core ", env!("FABER_CORE_BUILD"));

/// Key of one feature. It hashes the layout, the kernel version, the feature id, the feature
/// and the keys of its inputs. The stored face origins name the feature id, so the id is part of the key.
pub(crate) fn key(
    kernel_version: &str,
    id: &str,
    geometry: &impl std::fmt::Debug,
    inputs: &[String],
) -> String {
    let geometry = format!("{geometry:?}");
    let mut hash = Sha256::new();
    for part in [LAYOUT, kernel_version, id, geometry.as_str()] {
        hash.update(part.as_bytes());
        hash.update([0u8]);
    }
    for input in inputs {
        hash.update(input.as_bytes());
        hash.update([0u8]);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A stored feature: its face count, its face origins and the opaque shape bytes.
pub(crate) struct Entry<'a> {
    pub(crate) faces: u32,
    pub(crate) origins: Option<FaceOrigins>,
    pub(crate) shape: &'a [u8],
}

pub(crate) fn encode(faces: u32, origins: Option<&FaceOrigins>, shape: &[u8]) -> Vec<u8> {
    let mut out = faces.to_le_bytes().to_vec();
    match origins {
        None => out.push(0),
        Some(origins) => {
            out.push(1);
            out.extend_from_slice(&(origins.len() as u32).to_le_bytes());
            for (face, names) in origins {
                out.extend_from_slice(&face.to_le_bytes());
                out.extend_from_slice(&(names.len() as u32).to_le_bytes());
                for name in names {
                    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                    out.extend_from_slice(name.as_bytes());
                }
            }
        }
    }
    out.extend_from_slice(&(shape.len() as u64).to_le_bytes());
    out.extend_from_slice(shape);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < count {
            return None;
        }
        let (head, tail) = self.bytes.split_at(count);
        self.bytes = tail;
        Some(head)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let length = self.u32()? as usize;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }
}

/// Reads an entry. Any malformed byte returns None, and the feature is built again.
pub(crate) fn decode(bytes: &[u8]) -> Option<Entry<'_>> {
    let mut reader = Reader { bytes };
    let faces = reader.u32()?;
    let origins = match reader.take(1)?[0] {
        0 => None,
        1 => {
            let mut origins = FaceOrigins::new();
            for _ in 0..reader.u32()? {
                let face = reader.u32()?;
                let mut names = std::collections::BTreeSet::new();
                for _ in 0..reader.u32()? {
                    names.insert(reader.text()?);
                }
                origins.insert(face, names);
            }
            Some(origins)
        }
        _ => return None,
    };
    let length = u64::from_le_bytes(reader.take(8)?.try_into().ok()?) as usize;
    let shape = reader.take(length)?;
    if !reader.bytes.is_empty() {
        return None;
    }
    Some(Entry {
        faces,
        origins,
        shape,
    })
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn key_depends_on_every_part() {
        let inputs = ["in".to_string()];
        let base = key("v1", "a", &"box", &inputs);
        assert_eq!(base, key("v1", "a", &"box", &inputs));
        assert_ne!(base, key("v2", "a", &"box", &inputs), "kernel version");
        assert_ne!(base, key("v1", "b", &"box", &inputs), "feature id");
        assert_ne!(base, key("v1", "a", &"cube", &inputs), "geometry");
        assert_ne!(
            base,
            key("v1", "a", &"box", &["other".to_string()]),
            "input key"
        );
    }
}
