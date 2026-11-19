use super::*;

type Boundary = BTreeMap<u32, (Option<u32>, Option<u32>)>;

// Keep intermediate vertices in the same proportions along each straight boundary.
fn inset_grid(d: &mut Data, cap: &BTreeMap<u32, u32>, boundary: &Boundary, n: DVec3) -> Result<()> {
    let corner = |id: u32| {
        let (Some(a), Some(b)) = boundary[&id] else {
            return false;
        };
        let left = (point(d, id) - point(d, a)).normalize_or_zero();
        let right = (point(d, b) - point(d, id)).normalize_or_zero();
        left.dot(right) < 1. - 1e-10
    };
    let corners: BTreeSet<_> = boundary.keys().copied().filter(|&v| corner(v)).collect();
    for &start in &corners {
        let mut run = vec![];
        let mut v = boundary[&start].1.unwrap();
        while !corners.contains(&v) {
            run.push(v);
            v = boundary[&v].1.unwrap();
        }
        if run.is_empty() {
            continue;
        }
        let a = point(d, start);
        let b = point(d, v);
        let line = b - a;
        let from = point(d, cap[&start]);
        let to = point(d, cap[&v]);
        let length = line.length_squared();
        if length == 0. {
            return Err("The inset has coincident corners.".into());
        }
        for i in run {
            let t = (point(d, i) - a).dot(line) / length;
            d.vertices[cap[&i] as usize] = from.lerp(to, t).to_array();
        }
    }
    if cap.len() == boundary.len() {
        return Ok(());
    }
    // Fit the affine displacement of the boundary in its plane to the interior grid.
    let ux = n.any_orthonormal_vector();
    let uy = n.cross(ux);
    let count = boundary.len() as f64;
    let centre = boundary.keys().map(|&v| point(d, v)).sum::<DVec3>() / count;
    let shift = boundary
        .keys()
        .map(|&v| point(d, cap[&v]) - point(d, v))
        .sum::<DVec3>()
        / count;
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    let (mut dx, mut dy) = (DVec3::ZERO, DVec3::ZERO);
    for &v in boundary.keys() {
        let p = point(d, v);
        let (x, y) = ((p - centre).dot(ux), (p - centre).dot(uy));
        let delta = point(d, cap[&v]) - p - shift;
        xx += x * x;
        xy += x * y;
        yy += y * y;
        dx += x * delta;
        dy += y * delta;
    }
    let det = xx * yy - xy * xy;
    if det <= f64::EPSILON * xx * yy {
        return Err("The inset boundary has no area in its plane.".into());
    }
    let gx = (yy * dx - xy * dy) / det;
    let gy = (xx * dy - xy * dx) / det;
    for (&v, &top) in cap {
        if v == top {
            let p = point(d, v);
            d.vertices[v as usize] =
                (p + shift + gx * (p - centre).dot(ux) + gy * (p - centre).dot(uy)).to_array();
        }
    }
    Ok(())
}

pub(super) fn raise(
    d: &mut Data,
    selected: &BTreeSet<u32>,
    distance: f64,
    inset: bool,
) -> Result<()> {
    if !distance.is_finite() || distance == 0. || inset && distance < 0. {
        return Err("Use a finite nonzero extrusion or a positive inset distance.".into());
    }
    let mut shared = BTreeMap::<Edge, Vec<u32>>::new();
    for &f in selected {
        for [a, b] in perimeter(&d.faces[f as usize]) {
            shared.entry(edge(a, b)).or_default().push(f);
        }
    }
    let mut remaining = selected.clone();
    let old_creases = crease_map(d);
    let mut creases = old_creases.clone();
    while let Some(seed) = remaining.pop_first() {
        let mut faces = vec![seed];
        let mut i = 0;
        let mut boundary = Vec::new();
        let mut vertices = BTreeSet::new();
        let mut normal = DVec3::ZERO;
        while i < faces.len() {
            let face = &d.faces[faces[i] as usize];
            i += 1;
            normal += area_normal(d, face);
            for [a, b] in perimeter(face) {
                vertices.insert(a);
                let uses = &shared[&edge(a, b)];
                if uses.len() == 1 {
                    boundary.push([a, b]);
                } else {
                    for &f in uses {
                        if remaining.remove(&f) {
                            faces.push(f);
                        }
                    }
                }
            }
        }
        if boundary.is_empty() {
            return Err("The selected patch needs a boundary.".into());
        }
        if d.faces.len() + boundary.len() > crate::MAX_FACES {
            return Err("The cage exceeds 400000 faces.".into());
        }
        let n = normal
            .try_normalize()
            .ok_or("The selected patch has no mean normal.")?;
        let mut neighbours = Boundary::new();
        for &[a, b] in &boundary {
            if neighbours.entry(a).or_default().1.replace(b).is_some()
                || neighbours.entry(b).or_default().0.replace(a).is_some()
            {
                return Err("The selected patch touches itself at a vertex.".into());
            }
        }
        let original_normals: Vec<_> = if inset {
            faces
                .iter()
                .map(|&f| area_normal(d, &d.faces[f as usize]))
                .collect()
        } else {
            vec![]
        };
        let mut cap = BTreeMap::<u32, u32>::new();
        for v in vertices {
            let p = point(d, v);
            if let Some(&(Some(prev), Some(next))) = neighbours.get(&v) {
                let q = if inset {
                    let left = n
                        .cross(p - point(d, prev))
                        .try_normalize()
                        .ok_or("The inset edge has no direction in the patch plane.")?;
                    let right = n
                        .cross(point(d, next) - p)
                        .try_normalize()
                        .ok_or("The inset edge has no direction in the patch plane.")?;
                    let divisor = 1. + left.dot(right);
                    if divisor <= 1e-10 {
                        return Err("The inset boundary folds back on itself.".into());
                    }
                    p + (left + right) * (distance / divisor)
                } else {
                    p + n * distance
                };
                cap.insert(v, add(d, q));
            } else {
                if !inset {
                    d.vertices[v as usize] = (p + n * distance).to_array();
                }
                cap.insert(v, v);
            }
        }
        if inset {
            inset_grid(d, &cap, &neighbours, n)?;
        }
        for &[a, b] in &boundary {
            let [c, e] = [cap[&a], cap[&b]];
            if inset && (point(d, b) - point(d, a)).dot(point(d, e) - point(d, c)) <= 0. {
                return Err("The inset distance collapses the patch.".into());
            }
            d.faces.push(vec![a, b, e, c]);
        }
        for (index, f) in faces.into_iter().enumerate() {
            let face = &mut d.faces[f as usize];
            for [a, b] in perimeter(face) {
                if let Some(&s) = old_creases.get(&edge(a, b)) {
                    creases.insert(edge(cap[&a], cap[&b]), s);
                }
            }
            for v in face {
                *v = cap[v];
            }
            if inset && original_normals[index].dot(area_normal(d, &d.faces[f as usize])) <= 0. {
                return Err("The inset distance collapses a face in the patch.".into());
            }
        }
    }
    if !creases.is_empty() {
        let edges: BTreeSet<_> = d
            .faces
            .iter()
            .flat_map(|f| perimeter(f))
            .map(|[a, b]| edge(a, b))
            .collect();
        creases.retain(|e, _| edges.contains(e));
        set_creases(d, creases);
    }
    Ok(())
}
