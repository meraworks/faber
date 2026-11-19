use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use glam::{DMat3, DVec3};
use rayon::prelude::*;
use std::{
    collections::{HashMap, VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, OnceLock},
};

type Bake = Arc<Vec<[f32; 3]>>;
static BAKES: Mutex<VecDeque<(u64, Bake)>> = Mutex::new(VecDeque::new());
static POOL: OnceLock<Result<rayon::ThreadPool>> = OnceLock::new();
fn coordinates(mesh: &Mesh) -> Result<(DVec3, DMat3)> {
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|i| DVec3::from(mesh.vertices[i as usize]));
        if let (Some(x), Some(z)) = (
            (b - a).try_normalize(),
            (b - a).cross(c - a).try_normalize(),
        ) {
            return Ok((a, DMat3::from_cols(x, z.cross(x), z)));
        }
    }
    Err("The studio mesh has no triangle with area.".into())
}
/// Store the bake in the intrinsic frame of each mesh. Placement and colour do not change it.
pub(super) fn bake(mesh: &Mesh, normals: &[[[f32; 3]; 3]]) -> Result<Bake> {
    let pool = POOL.get_or_init(|| {
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get().saturating_sub(1).clamp(1, 4));
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|i| format!("studio-occlusion-{i}"))
            .build()
            .map_err(|e| format!("Could not start the studio workers. {e}"))
    });
    pool.as_ref()
        .map_err(Clone::clone)?
        .install(|| bake_mesh(mesh, normals))
}

fn bake_mesh(mesh: &Mesh, normals: &[[[f32; 3]; 3]]) -> Result<Bake> {
    let (origin, axes) = coordinates(mesh)?;
    let local = axes.transpose();
    let mut hash = DefaultHasher::new();
    mesh.triangles.hash(&mut hash);
    let key = |p: DVec3| p.to_array().map(|v| (v * 1e7).round() as i64);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = -lo;
    for &p in &mesh.vertices {
        let q = local * (DVec3::from(p) - origin);
        key(q).hash(&mut hash);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    for n in &mesh.normals {
        key(local * DVec3::from(*n)).hash(&mut hash);
    }
    let id = hash.finish();
    if let Some((_, v)) = BAKES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|(k, _)| *k == id)
    {
        return Ok(v.clone());
    }
    let bvh = Bvh::new(mesh)?;
    let range = (hi - lo).length() * 0.55;
    let bias = (range * 1e-7).max(1e-9);
    let mut unique = HashMap::new();
    let mut queries = vec![];
    let mut ids = Vec::with_capacity(mesh.triangles.len());
    for (&tri, ns) in mesh.triangles.iter().zip(normals) {
        let points = tri.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let centre = (points[0] + points[1] + points[2]) / 3.;
        ids.push(std::array::from_fn::<_, 3, _>(|k| {
            let key = (tri[k], ns[k].map(|c| (c * 100000.).round() as i32));
            *unique.entry(key).or_insert_with(|| {
                let index = queries.len();
                let n = glam::Vec3::from(ns[k]).as_dvec3();
                queries.push((
                    points[k] + n * bias + (centre - points[k]).normalize_or_zero() * bias * 4.,
                    n,
                ));
                index
            })
        }));
    }
    let rays: Vec<_> = (0..32)
        .map(|i| {
            let r = ((f64::from(i) + 0.5) / 32.).sqrt();
            let a = f64::from(i) * 2.399963229728653;
            DVec3::new(r * a.cos(), r * a.sin(), (1. - r * r).sqrt())
        })
        .collect();
    let values: Vec<_> = queries
        .par_iter()
        .map(|(p, n)| {
            #[cfg(test)]
            tests::WORKERS.fetch_max(
                rayon::current_num_threads(),
                std::sync::atomic::Ordering::Relaxed,
            );
            let Some(unit) = n.try_normalize() else {
                return 1.;
            };
            let ln = local * unit;
            let tangent = axes * ln.any_orthonormal_vector();
            let bitangent = n.cross(tangent);
            let blocked = rays
                .iter()
                .map(|r| {
                    let d = tangent * r.x + bitangent * r.y + *n * r.z;
                    bvh.first_hit(p.to_array(), d.to_array())
                        .map_or(0., |hit| (1. - hit.t / range).clamp(0., 1.))
                })
                .sum::<f64>();
            (1. - blocked / 32.) as f32
        })
        .collect();
    let result: Bake = Arc::new(ids.iter().map(|ids| ids.map(|i| values[i])).collect());
    let mut cache = BAKES.lock().unwrap_or_else(|p| p.into_inner());
    let mut total = result.len() + cache.iter().map(|(_, v)| v.len()).sum::<usize>();
    while total > 2_000_000 || cache.len() >= 128 {
        if let Some((_, old)) = cache.pop_front() {
            total -= old.len()
        } else {
            break;
        }
    }
    if result.len() <= 2_000_000 {
        cache.push_back((id, result.clone()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) static WORKERS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn ao_leaves_processors_available_for_the_viewer() {
        let mesh = Mesh {
            vertices: vec![[0., 0., 0.], [11., 0., 0.], [0., 15., 0.]],
            triangles: vec![[0, 1, 2]],
            normals: vec![],
        };
        let bake = bake(&mesh, &faber_scene::corner_normals(&mesh)).unwrap();
        assert_eq!(bake[0], [1.; 3]);
        let workers = WORKERS.load(std::sync::atomic::Ordering::Relaxed);
        assert!((1..=4).contains(&workers), "AO uses {workers} workers");
    }
    #[test]
    fn ao_moves_with_the_body_and_an_edit_rebuilds_it() {
        let mesh = Mesh {
            vertices: vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [1., 1., 0.],
                [0., 1., 0.],
                [0., 0., 1.],
                [1., 0., 1.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1]],
            normals: vec![],
        };
        let render = |mesh: &Mesh| bake(mesh, &faber_scene::corner_normals(mesh)).unwrap();
        let initial = render(&mesh);
        let mut moved = mesh.clone();
        let turn = glam::DQuat::from_axis_angle(DVec3::new(1., 3., 2.).normalize(), 0.913);
        for p in &mut moved.vertices {
            *p = (turn * DVec3::from(*p) + DVec3::new(15., -4., 99.)).into();
        }
        assert!(Arc::ptr_eq(&initial, &render(&moved)));
        moved.vertices[5][0] += 0.1;
        assert!(!Arc::ptr_eq(&initial, &render(&moved)));
    }
}
