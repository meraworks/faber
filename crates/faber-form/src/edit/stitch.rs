use super::*;

// Give an extra side of a quad one triangle where it meets a triangle.
fn matched(f: &[u32], length: usize, start: usize, repeat: usize, reverse: bool) -> Vec<u32> {
    (0..length)
        .map(|i| {
            let j = if length > f.len() && i > repeat {
                i - 1
            } else {
                i
            };
            let index = if reverse {
                start + f.len() - j
            } else {
                start + j
            };
            f[index % f.len()]
        })
        .collect()
}
fn tube(d: &Data, first: &[u32], second: &[u32]) -> Result<Vec<Vec<u32>>> {
    if first.iter().any(|v| second.contains(v)) {
        return Err("Bridge faces must have separate vertices.".into());
    }
    let n = first.len().max(second.len());
    let mut best = None;
    let mut cost = f64::INFINITY;
    let repeats = |k: usize| if k < n { k } else { 1 };
    for i in 0..repeats(first.len()) {
        let a = matched(first, n, 0, i, false);
        for j in 0..second.len() {
            for k in 0..repeats(second.len()) {
                let b = matched(second, n, j, k, true);
                let length = a
                    .iter()
                    .zip(&b)
                    .map(|(&u, &v)| point(d, u).distance_squared(point(d, v)))
                    .sum::<f64>();
                if length < cost {
                    cost = length;
                    best = Some((a.clone(), b));
                }
            }
        }
    }
    let (a, b) = best.ok_or("The bridge dimensions exceed the numeric range.")?;
    Ok((0..n)
        .map(|i| {
            let j = (i + 1) % n;
            let mut wall = vec![a[i], a[j], b[j], b[i]];
            wall.dedup();
            wall
        })
        .collect())
}
pub(super) fn bridge(d: &mut Data, m: &Mirrors, a: u32, b: u32) -> Result<()> {
    let mut pairs = BTreeSet::new();
    for r in &m.maps {
        pairs.insert(edge(m.face(d, r, a)?, m.face(d, r, b)?));
    }
    let mut removed = BTreeSet::new();
    let mut walls = vec![];
    for [a, b] in pairs {
        if !removed.insert(a) || !removed.insert(b) {
            return Err("Each bridge needs two different faces.".into());
        }
        walls.extend(tube(d, &d.faces[a as usize], &d.faces[b as usize])?);
    }
    let mut id = 0;
    d.faces.retain(|_| {
        id += 1;
        !removed.contains(&(id - 1))
    });
    d.faces.extend(walls);
    Ok(())
}

pub(super) fn compact(d: &mut Data, origins: Option<&mut Vec<Option<usize>>>) {
    let mut map = vec![u32::MAX; d.vertices.len()];
    for &id in d.faces.iter().flatten() {
        map[id as usize] = 0;
    }
    let mut next = 0;
    let mut old = 0;
    d.vertices.retain(|_| {
        let keep = map[old] != u32::MAX;
        if keep {
            map[old] = next;
            next += 1;
        }
        old += 1;
        keep
    });
    if let Some(origins) = origins {
        let mut index = 0;
        origins.retain(|_| {
            let keep = map[index] != u32::MAX;
            index += 1;
            keep
        });
    }
    for v in d.faces.iter_mut().flatten() {
        *v = map[*v as usize];
    }
    let used: BTreeSet<_> = d
        .faces
        .iter()
        .flat_map(|f| perimeter(f))
        .map(|[a, b]| edge(a, b))
        .collect();
    let mut kept = BTreeMap::<Edge, f64>::new();
    for crease in &d.creases {
        let [a, b] = crease.edge.map(|v| map[v as usize]);
        let e = edge(a, b);
        if used.contains(&e) {
            let s = kept.entry(e).or_default();
            *s = s.max(crease.sharpness);
        }
    }
    set_creases(d, kept);
}
pub(super) fn weld(
    d: &mut Data,
    m: &Mirrors,
    a: u32,
    b: u32,
    mut origins: Option<&mut Vec<Option<usize>>>,
) -> Result<()> {
    for v in [a, b] {
        check_index(v, d.vertices.len(), "vertex")?;
    }
    if a == b {
        return Err("Choose two different vertices to weld.".into());
    }
    let mut parent: BTreeMap<u32, u32> = BTreeMap::new();
    let root = |parents: &BTreeMap<u32, u32>, mut x| {
        while let Some(&p) = parents.get(&x) {
            x = p;
        }
        x
    };
    let mut affected = BTreeSet::new();
    for r in &m.maps {
        let [u, v] = [r.vertex(a), r.vertex(b)];
        affected.extend([u, v]);
        let [u, v] = [root(&parent, u), root(&parent, v)];
        if u != v {
            parent.insert(v, u);
        }
    }
    let mut groups = BTreeMap::<u32, Vec<u32>>::new();
    for v in affected {
        groups.entry(root(&parent, v)).or_default().push(v);
    }
    for (&v, ids) in &groups {
        if let Some(origins) = origins.as_mut() {
            origins[v as usize] = None;
        }
        let centre = ids.iter().map(|&i| point(d, i)).sum::<DVec3>() / ids.len() as f64;
        d.vertices[v as usize] = centre.to_array();
    }
    for face in &mut d.faces {
        for i in face.iter_mut() {
            *i = root(&parent, *i);
        }
        face.dedup();
        if face.len() > 1 && face.first() == face.last() {
            face.pop();
        }
        if face.iter().collect::<BTreeSet<_>>().len() != face.len() {
            return Err("The weld pinches a face at a vertex.".into());
        }
    }
    d.faces.retain(|f| f.len() >= 3);
    for c in &mut d.creases {
        c.edge = c.edge.map(|v| root(&parent, v));
    }
    compact(d, origins);
    Ok(())
}
