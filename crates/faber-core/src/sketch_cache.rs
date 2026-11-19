//! Persist only converged sketch solutions. The feature key fixes the point and entity order.
use faber_ports::{FeatureCache, Result};
use faber_sketch::{Entity, Sketch, Solution, Status};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const LIMIT: usize = 32 * 1024;
const MAGIC: &[u8; 4] = b"FSS1";

pub(crate) fn get_or_solve(
    store: Option<&dyn FeatureCache>,
    key: Option<&str>,
    sketch: &Sketch,
    solve: impl FnOnce(&Sketch) -> Result<Solution>,
) -> Result<Solution> {
    let Some((store, key)) = store.zip(key) else {
        return solve(sketch);
    };
    let address = format!("{key}sketch1");
    if let Some(bytes) = store.get_bounded(&address, LIMIT)
        && let Some(result) = decode(&address, &bytes, sketch)
    {
        return Ok(result);
    }
    let result = solve(sketch)?;
    if store.writable() {
        let bytes = encode(&address, &result);
        if bytes.len() <= LIMIT {
            let _ = store.put(&address, &bytes);
        }
    }
    Ok(result)
}

fn digest(key: &str, payload: &[u8]) -> [u8; 32] {
    Sha256::new()
        .chain_update(key.as_bytes())
        .chain_update(payload)
        .finalize()
        .into()
}

fn encode(key: &str, s: &Solution) -> Vec<u8> {
    let mut data = Vec::new();
    // The solver limits points to 320, variables to 640 and constraints to 1000.
    for n in [
        s.points.len(),
        s.radii.len(),
        s.dof,
        s.iterations,
        s.redundant.len(),
    ] {
        data.extend_from_slice(&(n as u16).to_le_bytes());
    }
    data.extend_from_slice(&s.residual.to_le_bytes());
    for (name, [x, y]) in &s.points {
        data.extend_from_slice(&x.to_le_bytes());
        data.extend_from_slice(&y.to_le_bytes());
        data.push(u8::from(s.free_points.binary_search(name).is_ok()));
    }
    for (name, r) in &s.radii {
        data.extend_from_slice(&r.to_le_bytes());
        data.push(u8::from(s.free_radii.binary_search(name).is_ok()));
    }
    for &i in &s.redundant {
        data.extend_from_slice(&(i as u16).to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(36 + data.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&digest(key, &data));
    bytes.extend(data);
    bytes
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (a, b) = self.0.split_at_checked(N)?;
        self.0 = b;
        a.try_into().ok()
    }
    fn count(&mut self) -> Option<usize> {
        Some(u16::from_le_bytes(self.take()?) as usize)
    }
    fn scalar(&mut self) -> Option<f64> {
        let v = f64::from_le_bytes(self.take()?);
        v.is_finite().then_some(v)
    }
    fn free(&mut self) -> Option<bool> {
        match self.take()? {
            [0] => Some(false),
            [1] => Some(true),
            _ => None,
        }
    }
}

fn decode(key: &str, bytes: &[u8], sketch: &Sketch) -> Option<Solution> {
    if bytes.len() > LIMIT {
        return None;
    }
    let (header, payload) = bytes.split_at_checked(36)?;
    if &header[..4] != MAGIC || header[4..] != digest(key, payload) {
        return None;
    }
    let mut input = Reader(payload);
    let npoints = input.count()?;
    let nradii = input.count()?;
    let dof = input.count()?;
    let iterations = input.count()?;
    let nredundant = input.count()?;
    let residual = input.scalar()?;
    let ncircles = sketch
        .entities
        .values()
        .filter(|e| matches!(e, Entity::Circle { .. }))
        .count();
    let variables = 2 * npoints + ncircles;
    if npoints != sketch.points.len()
        || npoints > 320
        || variables > 640
        || dof > variables
        || iterations > 100
        || nredundant > sketch.constraints.len()
        || !(0. ..1e-9).contains(&residual)
    {
        return None;
    }
    let mut points = BTreeMap::new();
    let mut free_points = Vec::new();
    for name in sketch.points.keys() {
        points.insert(name.clone(), [input.scalar()?, input.scalar()?]);
        if input.free()? {
            free_points.push(name.clone());
        }
    }
    let mut radii = BTreeMap::new();
    let mut free_radii = Vec::new();
    for (name, entity) in &sketch.entities {
        if matches!(entity, Entity::Line { .. }) {
            continue;
        }
        let r = input.scalar()?;
        if r <= 0. {
            return None;
        }
        radii.insert(name.clone(), r);
        if input.free()? {
            if !matches!(entity, Entity::Circle { .. }) {
                return None;
            }
            free_radii.push(name.clone());
        }
    }
    if radii.len() != nradii || (dof == 0) != (free_points.is_empty() && free_radii.is_empty()) {
        return None;
    }
    let mut redundant = Vec::with_capacity(nredundant);
    for _ in 0..nredundant {
        let i = input.count()?;
        if i >= sketch.constraints.len() || redundant.contains(&i) {
            return None;
        }
        redundant.push(i);
    }
    if !input.0.is_empty() {
        return None;
    }
    Some(Solution {
        points,
        radii,
        dof,
        status: if dof == 0 {
            Status::Solved
        } else {
            Status::Under
        },
        free_points,
        free_radii,
        redundant,
        iterations,
        residual,
        diagnosis_limit: None,
        conflicting: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, Vec<u8>>>);
    impl FeatureCache for Memory {
        fn get(&self, key: &str) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn put(&self, key: &str, data: &[u8]) -> Result<()> {
            self.0.lock().unwrap().insert(key.into(), data.into());
            Ok(())
        }
    }
    fn circle() -> Sketch {
        Sketch {
            points: [("p".into(), [0.1 + 0.2, 4.])].into(),
            entities: [(
                "rim".into(),
                Entity::Circle {
                    centre: "p".into(),
                    radius: 2.,
                },
            )]
            .into(),
            ..Default::default()
        }
    }
    #[test]
    fn a_solution_hit_uses_no_solver_and_keeps_the_coordinate_bits() {
        let cache = Memory::default();
        let sketch = circle();
        let first =
            get_or_solve(Some(&cache), Some("feature"), &sketch, crate::sketch::solve).unwrap();
        let read = get_or_solve(Some(&cache), Some("feature"), &sketch, |_| {
            panic!("cache hit called the solver")
        })
        .unwrap();
        assert_eq!(
            first.points["p"].map(f64::to_bits),
            read.points["p"].map(f64::to_bits)
        );
        assert_eq!(first.free_radii, read.free_radii);
        assert_eq!(first.free_points, read.free_points);
        assert_eq!(first.radii, read.radii);
        assert_eq!(first.dof, read.dof);
        let bytes = cache.get("featuresketch1").unwrap();
        assert!(decode("differentfeaturesketch1", &bytes, &sketch).is_none());
    }
    #[test]
    fn malformed_diagnostics_and_nonfinite_points_need_a_new_solve() {
        let sketch = circle();
        let key = "featuresketch1";
        let initial = crate::sketch::solve(&sketch).unwrap();
        let mut bad = initial.clone();
        bad.points.get_mut("p").unwrap()[1] = f64::INFINITY;
        assert!(decode(key, &encode(key, &bad), &sketch).is_none());
        let mut bad = initial.clone();
        bad.redundant.push(99);
        assert!(decode(key, &encode(key, &bad), &sketch).is_none());
        let mut bad = initial;
        bad.radii.clear();
        assert!(decode(key, &encode(key, &bad), &sketch).is_none());
    }
}
