use super::*;
use std::collections::VecDeque;

pub(super) fn insert(d: &mut Data, mirrors: &Mirrors, seed: Edge, at: f64) -> Result<()> {
    if !at.is_finite() || at <= 0. || at >= 1. {
        return Err("Place the loop strictly between the edge ends.".into());
    }
    let mut uses = BTreeMap::<Edge, Vec<(usize, usize)>>::new();
    for (face, f) in d.faces.iter().enumerate() {
        for (side, [a, b]) in perimeter(f).enumerate() {
            uses.entry(edge(a, b)).or_default().push((face, side));
        }
    }
    let mut cuts = BTreeMap::<Edge, Vec<f64>>::new();
    let mut queue = VecDeque::new();
    for r in &mirrors.maps {
        queue.push_back((r.vertex(seed[0]), r.vertex(seed[1]), at));
    }
    while let Some((a, b, t)) = queue.pop_front() {
        let e = edge(a, b);
        let s = if a == e[0] { t } else { 1. - t };
        let stops = cuts.entry(e).or_default();
        if stops.iter().any(|q| (s - q).abs() < 1e-12) {
            continue;
        }
        stops.push(s);
        for &(face, i) in &uses[&e] {
            let f = &d.faces[face];
            if f.len() != 4 {
                return Err(format!(
                    "The edge ring reaches face {face}. A loop needs quadrilaterals."
                ));
            }
            let opp = [f[(i + 3) % 4], f[(i + 2) % 4]];
            queue.push_back((opp[0], opp[1], if f[i] == a { t } else { 1. - t }));
        }
    }
    let added_edges = cuts.values().map(Vec::len).sum::<usize>();
    let interior: usize = d
        .faces
        .iter()
        .filter(|f| f.len() == 4)
        .map(|f| {
            let u = cuts.get(&edge(f[0], f[1])).map_or(0, Vec::len);
            let v = cuts.get(&edge(f[0], f[3])).map_or(0, Vec::len);
            u * v
        })
        .sum();
    if d.faces.len() + added_edges + interior > crate::MAX_FACES {
        return Err("The cage exceeds 400000 faces.".into());
    }
    let mut points = BTreeMap::<Edge, Vec<(f64, u32)>>::new();
    let mut creases = crease_map(d);
    for ([a, b], mut stops) in cuts {
        stops.sort_by(f64::total_cmp);
        let mut row = vec![];
        let start = point(d, a);
        let end = point(d, b);
        let sharp = creases.remove(&[a, b]);
        let mut prev = a;
        for t in stops {
            let v = add(d, start.lerp(end, t));
            row.push((t, v));
            if let Some(s) = sharp {
                creases.insert(edge(prev, v), s);
            }
            prev = v;
        }
        if let Some(s) = sharp {
            creases.insert(edge(prev, b), s);
        }
        points.insert([a, b], row);
    }
    let stops = |a, b| {
        let mut v = vec![(0., a)];
        if let Some(row) = points.get(&edge(a, b)) {
            if a < b {
                v.extend(row.iter().copied());
            } else {
                v.extend(row.iter().rev().map(|&(t, id)| (1. - t, id)));
            }
        }
        v.push((1., b));
        v
    };
    let original = d.faces.len();
    for i in 0..original {
        let f = &d.faces[i];
        if !perimeter(f).any(|[a, b]| points.contains_key(&edge(a, b))) {
            continue;
        }
        let [a, b, c, e] = [f[0], f[1], f[2], f[3]];
        let bottom = stops(a, b);
        let top = stops(e, c);
        let left = stops(a, e);
        let right = stops(b, c);
        if bottom.len() != top.len() || left.len() != right.len() {
            return Err("The edge ring does not close.".into());
        }
        let w = bottom.len();
        let h = left.len();
        let mut grid = vec![0; w * h];
        for (y, &(v, l)) in left.iter().enumerate() {
            for (x, &(u, bt)) in bottom.iter().enumerate() {
                grid[y * w + x] = if y == 0 {
                    bt
                } else if y == h - 1 {
                    top[x].1
                } else if x == 0 {
                    l
                } else if x == w - 1 {
                    right[y].1
                } else {
                    let p = point(d, a)
                        .lerp(point(d, b), u)
                        .lerp(point(d, e).lerp(point(d, c), u), v);
                    add(d, p)
                };
            }
        }
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let j = y * w + x;
                let new_face = vec![grid[j], grid[j + 1], grid[j + w + 1], grid[j + w]];
                if j == 0 {
                    d.faces[i] = new_face;
                } else {
                    d.faces.push(new_face);
                }
            }
        }
    }
    set_creases(d, creases);
    Ok(())
}
