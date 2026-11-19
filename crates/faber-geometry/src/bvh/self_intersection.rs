use super::{Bvh, Children, Triangle};
use glam::DVec3;

fn crosses(a: &Triangle, b: &Triangle) -> bool {
    let shared = a.indices.iter().filter(|i| b.indices.contains(i)).count();
    let edges = |p: [DVec3; 3]| [p[1] - p[0], p[2] - p[1], p[0] - p[2]];
    let origin = a.points[0];
    let local = |tri: &Triangle| tri.points.map(|p| p - origin);
    let (p, q) = (local(a), local(b));
    let (ea, eb) = (edges(p), edges(q));
    let (Some(na), Some(nb)) = (
        ea[0].cross(ea[1]).try_normalize(),
        eb[0].cross(eb[1]).try_normalize(),
    ) else {
        return false;
    };
    let scale = ea.iter().chain(&eb).map(|e| e.length()).fold(0., f64::max);
    let tolerance = scale * 1e-12;
    if shared == 3 {
        return true;
    }
    let line = na.cross(nb);
    if shared == 2 {
        if line.length() > 64. * f64::EPSILON {
            return false;
        }
        let i = a
            .indices
            .iter()
            .position(|v| !b.indices.contains(v))
            .unwrap();
        let j = b
            .indices
            .iter()
            .position(|v| !a.indices.contains(v))
            .unwrap();
        let start = p[(i + 1) % 3];
        let across = na.cross(p[(i + 2) % 3] - start);
        return across.dot(p[i] - start).is_sign_positive()
            == across.dot(q[j] - start).is_sign_positive();
    }
    if shared == 1 {
        // The common corner is always a contact. Look for a ray inside both triangles.
        let i = a
            .indices
            .iter()
            .position(|v| b.indices.contains(v))
            .unwrap();
        let j = b.indices.iter().position(|v| *v == a.indices[i]).unwrap();
        let sides = |t: [DVec3; 3], i: usize| {
            [
                (t[(i + 1) % 3] - t[i]).normalize(),
                (t[(i + 2) % 3] - t[i]).normalize(),
            ]
        };
        let (u, v) = (sides(p, i), sides(q, j));
        let inside = |s: [DVec3; 2], n: DVec3, ray: DVec3| {
            s[0].cross(ray).dot(n) >= -64. * f64::EPSILON
                && ray.cross(s[1]).dot(n) >= -64. * f64::EPSILON
        };
        return if line.length() > 64. * f64::EPSILON {
            let ray = line.normalize();
            [ray, -ray]
                .into_iter()
                .any(|d| inside(u, na, d) && inside(v, nb, d))
        } else {
            u.into_iter().any(|d| inside(v, nb, d)) || v.into_iter().any(|d| inside(u, na, d))
        };
    }
    let separated = |axis: DVec3| {
        let Some(axis) = axis.try_normalize() else {
            return false;
        };
        let interval = |t: [DVec3; 3]| {
            t.iter()
                .map(|v| v.dot(axis))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v), hi.max(v))
                })
        };
        let (ap, aq) = (interval(p), interval(q));
        ap.1 < aq.0 - tolerance || aq.1 < ap.0 - tolerance
    };
    if separated(na) || separated(nb) {
        return false;
    }
    // In-plane axes also separate coplanar triangles.
    for e in ea {
        if separated(na.cross(e)) || eb.iter().any(|f| separated(e.cross(*f))) {
            return false;
        }
    }
    !eb.iter().any(|f| separated(nb.cross(*f)))
}

impl Bvh {
    /// Check non-neighbouring surface contacts on the mesh used to build this tree.
    pub fn intersects_itself(&self) -> bool {
        let mut pairs = vec![(0, 0)];
        while let Some((a, b)) = pairs.pop() {
            let (left, right) = (&self.nodes[a], &self.nodes[b]);
            if left.bounds.high.cmplt(right.bounds.low).any()
                || right.bounds.high.cmplt(left.bounds.low).any()
            {
                continue;
            }
            match (&left.children, &right.children) {
                (Children::Leaf(x), Children::Leaf(y)) => {
                    for i in x.clone() {
                        for j in y.clone() {
                            if (a != b || i < j) && crosses(&self.triangles[i], &self.triangles[j])
                            {
                                return true;
                            }
                        }
                    }
                }
                (Children::Pair(x, y), _) if a == b => pairs.extend([(*x, *x), (*x, *y), (*y, *y)]),
                (Children::Pair(x, y), _) => pairs.extend([(*x, b), (*y, b)]),
                (_, Children::Pair(x, y)) => pairs.extend([(a, *x), (a, *y)]),
            }
        }
        false
    }
}
