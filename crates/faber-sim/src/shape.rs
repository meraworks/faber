//! Density optimisation for one linear elastic load case.
use crate::{
    Grid, Load, StaticCase, Support, cells_near, element_matrix,
    solve::{corners, displacements, xyz},
    surface::finish_mesh,
};
use faber_ports::{Mesh, Result};
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}
#[derive(Clone, Debug)]
pub struct ShapeCase {
    pub base: StaticCase,
    pub keep: Vec<usize>,
    pub target_volume: f64,
    pub min_member_mm: f64,
    pub max_iterations: usize,
    pub symmetry: Vec<Plane>,
    pub density_g_cm3: f64,
}
#[derive(Debug)]
pub struct ShapeResult {
    pub density: Vec<f32>,
    pub volume_share: f64,
    pub compliance: Vec<f64>,
    pub mesh: Mesh,
    pub pieces: usize,
    pub iterations: usize,
    pub mass_g: f64,
    pub limits: Vec<String>,
}

fn marked_cells(case: &ShapeCase) -> Result<Vec<bool>> {
    let g = &case.base.grid;
    let mut keep = vec![false; g.fill.len()];
    let mut mark = |cells: &[usize]| -> Result<()> {
        for &i in cells {
            if g.fill.get(i).is_none_or(|&v| v < 0.02) {
                return Err("A kept cell is outside the part.".into());
            }
            keep[i] = true;
        }
        Ok(())
    };
    mark(&case.keep)?;
    for bc in &case.base.supports {
        match bc {
            Support::Fixed { cells } | Support::Slide { cells, .. } => mark(cells)?,
            Support::FixedFace { triangles } | Support::SlideFace { triangles, .. } => {
                mark(&cells_near(g, triangles))?
            }
        }
    }
    for load in &case.base.loads {
        match load {
            Load::Force { cells, .. } => mark(cells)?,
            Load::Traction { triangles, .. } | Load::Pressure { triangles, .. } => {
                mark(&cells_near(g, triangles))?
            }
            Load::Gravity { .. } => {}
            Load::Temperature { .. } => {
                return Err("Shape optimisation needs a mechanical load.".into());
            }
        }
    }
    Ok(keep)
}

fn root_of(groups: &mut [usize], mut i: usize) -> usize {
    while groups[i] != i {
        groups[i] = groups[groups[i]];
        i = groups[i];
    }
    i
}
fn symmetry_groups(g: &Grid, planes: &[Plane]) -> Result<Vec<usize>> {
    let mut groups: Vec<usize> = (0..g.fill.len()).collect();
    for plane in planes {
        let normal = DVec3::from(plane.normal)
            .try_normalize()
            .ok_or("The symmetry plane needs a finite normal.")?;
        let origin = DVec3::from(plane.origin);
        if !normal.is_finite() || !origin.is_finite() {
            return Err("The symmetry plane must be finite.".into());
        }
        for (i, &f) in g.fill.iter().enumerate().filter(|(_, f)| **f >= 0.02) {
            let p = DVec3::from(g.origin)
                + g.h * (DVec3::from(xyz(i, g.n).map(|v| v as f64)) + DVec3::splat(0.5));
            let mirror = p - 2. * (p - origin).dot(normal) * normal;
            let q = ((mirror - DVec3::from(g.origin)) / g.h).floor();
            if q.min_element() < 0. || !q.cmplt(DVec3::from(g.n.map(|v| v as f64))).all() {
                return Err("The symmetry plane reflects material outside the part.".into());
            }
            let [x, y, z] = q.to_array().map(|v| v as usize);
            let j = x + g.n[0] * (y + g.n[1] * z);
            if g.fill[j] < 0.02 && f >= 0.02 {
                return Err("The symmetry plane reflects material outside the part.".into());
            }
            let a = root_of(&mut groups, i);
            let b = root_of(&mut groups, j);
            groups[a] = b;
        }
    }
    for i in 0..groups.len() {
        groups[i] = root_of(&mut groups, i);
    }
    Ok(groups)
}

struct Filter {
    offsets: Vec<([isize; 3], f64)>,
}
impl Filter {
    fn new(g: &Grid, radius: f64) -> Result<Self> {
        let r = radius / g.h;
        let span =
            g.n.map(|size| (r.ceil() as usize).min(size.saturating_sub(1)) as isize);
        let checks =
            span.iter().map(|&s| (2 * s + 1) as u128).product::<u128>() * g.fill.len() as u128;
        if checks > 2_000_000_000 {
            return Err(format!(
                "The filter needs {checks} cell visits per iteration. The limit is 2000000000. Reduce the filter radius."
            ));
        }
        let mut offsets = Vec::new();
        for z in -span[2]..=span[2] {
            for y in -span[1]..=span[1] {
                for x in -span[0]..=span[0] {
                    let delta = [x, y, z];
                    let w = 1. - DVec3::from(delta.map(|v| v as f64)).length() / r;
                    if w > 0. {
                        offsets.push((delta, w));
                    }
                }
            }
        }
        Ok(Self { offsets })
    }
    fn apply(&self, g: &Grid, input: &[f64]) -> Vec<f64> {
        g.fill
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                if f < 0.02 {
                    return 0.;
                }
                let [x, y, z] = xyz(i, g.n).map(|v| v as isize);
                let mut sum = 0.;
                let mut weight = 0.;
                for &([dx, dy, dz], w) in &self.offsets {
                    let p = [x + dx, y + dy, z + dz].map(|v| v as usize);
                    if (0..3).any(|a| p[a] >= g.n[a]) {
                        continue;
                    }
                    let j = p[0] + g.n[0] * (p[1] + g.n[1] * p[2]);
                    let v = w * f64::from(g.fill[j]);
                    sum += v * input[j];
                    weight += v;
                }
                sum / weight
            })
            .collect()
    }
}

pub fn optimise_shape(case: &ShapeCase) -> Result<ShapeResult> {
    optimise_shape_with_progress(case, |_, _, _| true)
}

/// Return false after an iteration to keep its density and surface.
pub fn optimise_shape_with_progress(
    case: &ShapeCase,
    mut progress: impl FnMut(usize, &[f32], f64) -> bool,
) -> Result<ShapeResult> {
    optimise_shape_with_poll(case, |i, done| {
        done.is_none_or(|(rho, c)| progress(i, rho, c))
    })
}

/// No field is sent while the inner linear solver checks for a stop request.
pub fn optimise_shape_with_poll(
    case: &ShapeCase,
    mut progress: impl FnMut(usize, Option<(&[f32], f64)>) -> bool,
) -> Result<ShapeResult> {
    let g = &case.base.grid;
    if !(0.1..=0.9).contains(&case.target_volume) {
        return Err("The target volume fraction must be between 0.1 and 0.9.".into());
    }
    if !case.min_member_mm.is_finite() || case.min_member_mm <= 0. || !g.h.is_finite() || g.h <= 0.
    {
        return Err("The member size and the cell size must be finite and positive.".into());
    }
    if !(1..=1000).contains(&case.max_iterations) {
        return Err("Shape optimisation needs between 1 and 1000 iterations.".into());
    }
    if !case.density_g_cm3.is_finite() || case.density_g_cm3 <= 0. {
        return Err("Material density must be finite and positive.".into());
    }
    if g.n.contains(&0)
        || g.n.iter().try_fold(1usize, |a, &b| a.checked_mul(b)) != Some(g.fill.len())
        || g.fill.len() > crate::CELL_LIMIT
        || g.fill.iter().any(|v| !(0.0..=1.).contains(v))
    {
        return Err("Invalid shape grid.".into());
    }
    let groups = symmetry_groups(g, &case.symmetry)?;
    let mut keep = marked_cells(case)?;
    let mut anchored = keep.clone();
    for (i, &on) in keep.iter().enumerate() {
        if on {
            anchored[groups[i]] = true;
        }
    }
    for (i, on) in keep.iter_mut().enumerate() {
        *on = anchored[groups[i]];
    }
    let total = g
        .fill
        .iter()
        .filter(|&&v| v >= 0.02)
        .map(|&v| f64::from(v))
        .sum::<f64>();
    if total <= 0. {
        return Err("The grid has no solid cells.".into());
    }
    let fixed = keep
        .iter()
        .zip(&g.fill)
        .filter(|(k, _)| **k)
        .map(|(_, f)| f64::from(*f))
        .sum::<f64>();
    let target = case.target_volume * total;
    if fixed > target {
        return Err(format!(
            "The kept regions are {:.0} % of the part. The target is {:.0} %.",
            fixed / total * 100.,
            case.target_volume * 100.
        ));
    }
    let free_share = (target - fixed) / (total - fixed);
    if free_share < 0.001 {
        return Err("The movable cells cannot meet the target at their minimum density.".into());
    }
    let filter = Filter::new(g, case.min_member_mm)?;
    let mut rho = keep
        .iter()
        .zip(&g.fill)
        .map(|(&keep, &f)| {
            if f < 0.02 {
                0.
            } else if keep {
                1.
            } else {
                free_share
            }
        })
        .collect::<Vec<f64>>();
    let ke = element_matrix(&case.base.material, g.h);
    let mut compliance = Vec::new();
    let mut start = None;
    let mut last_change = 1.;
    let mut last_complete = None;
    let mut sums = vec![0.; rho.len()];
    let mut group_weight = vec![0.; rho.len()];
    for (i, &f) in g.fill.iter().enumerate() {
        group_weight[groups[i]] += f64::from(f);
    }
    for iter in 1..=case.max_iterations {
        let scale = rho
            .iter()
            .zip(&g.fill)
            .map(|(&x, &f)| (x.powi(3) * f64::from(f)) as f32)
            .collect::<Vec<_>>();
        let mut stopped = false;
        let field = match displacements(&case.base, Some(&scale), start.as_deref(), &mut |_, _| {
            stopped = !progress(iter, None);
            !stopped
        }) {
            Ok(field) => field,
            Err(_) if stopped && last_complete.is_some() => {
                rho = last_complete.take().unwrap();
                break;
            }
            Err(error) => return Err(error),
        };
        let mut objective = 0.;
        let mut grad = vec![0.; rho.len()];
        for (i, &f) in g.fill.iter().enumerate().filter(|(_, f)| **f >= 0.02) {
            let nodes = corners(g, i);
            let u = std::array::from_fn::<_, 24, _>(|k| field.displacement[nodes[k / 3]][k % 3]);
            let energy = ke
                .iter()
                .zip(u)
                .map(|(row, v)| v * row.iter().zip(u).map(|(q, t)| q * t).sum::<f64>())
                .sum::<f64>()
                .max(0.);
            objective += rho[i].powi(3) * f64::from(f) * energy;
            grad[i] = 3. * rho[i].powi(3) * energy;
        }
        if !(objective > 0. && objective.is_finite()) {
            return Err("Shape optimisation needs a load that does work on the part.".into());
        }
        compliance.push(objective);
        let snapshot = rho.iter().map(|&v| v as f32).collect::<Vec<_>>();
        if !progress(iter, Some((&snapshot, objective)))
            || iter == case.max_iterations
            || last_change < 0.01
        {
            break;
        }
        start = Some(field.displacement);
        let mut sensitivity = filter.apply(g, &grad);
        sums.fill(0.);
        for (i, s) in sensitivity.iter_mut().enumerate() {
            if rho[i] > 0. {
                *s /= rho[i];
                sums[groups[i]] += *s * f64::from(g.fill[i]);
            }
        }
        for (i, s) in sensitivity.iter_mut().enumerate() {
            if rho[i] > 0. {
                *s = sums[groups[i]] / group_weight[groups[i]];
            }
        }
        let update = |i: usize, lambda: f64| {
            if keep[i] || rho[i] == 0. {
                rho[i]
            } else {
                (rho[i] * (sensitivity[i] / lambda).sqrt())
                    .clamp((rho[i] - 0.2).max(0.001), (rho[i] + 0.2).min(1.))
            }
        };
        let mut low = 0.;
        let mut high = sensitivity
            .iter()
            .copied()
            .fold(0., f64::max)
            .max(f64::MIN_POSITIVE)
            * 1e7;
        for _ in 0..80 {
            let mid = 0.5 * (low + high);
            let v = (0..rho.len())
                .map(|i| update(i, mid) * f64::from(g.fill[i]))
                .sum::<f64>();
            if v > target {
                low = mid;
            } else {
                high = mid;
            }
        }
        let next = (0..rho.len()).map(|i| update(i, high)).collect::<Vec<_>>();
        last_change = next
            .iter()
            .zip(&rho)
            .map(|(a, b)| (a - b).abs())
            .fold(0., f64::max);
        last_complete = Some(std::mem::replace(&mut rho, next));
    }
    let volume = rho
        .iter()
        .zip(&g.fill)
        .map(|(x, &f)| x * f64::from(f))
        .sum::<f64>();
    let mut filtered = filter.apply(g, &rho);
    for (i, v) in filtered.iter_mut().enumerate() {
        if keep[i] {
            *v = 1.;
        }
        *v *= f64::from(g.fill[i]);
    }
    let (mesh, pieces) = finish_mesh(
        g,
        &filtered.iter().map(|&x| x as f32).collect::<Vec<_>>(),
        &keep,
    );
    let mut limits = vec![
        "The result is a mesh. Model the final part over it or convert it to a solid.".into(),
        "Check the final part with a static study.".into(),
    ];
    if pieces > 1 {
        limits.push(format!("The result has {pieces} separate pieces."));
    }
    if pieces == 0 {
        limits.push("No material reaches the surface density of 0.5.".into());
    }
    Ok(ShapeResult {
        density: rho.into_iter().map(|x| x as f32).collect(),
        mass_g: volume * g.h.powi(3) * case.density_g_cm3 / 1000.,
        volume_share: volume / total,
        iterations: compliance.len(),
        mesh,
        pieces,
        compliance,
        limits,
    })
}
