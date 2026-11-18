use faber_model::Clearance;
use faber_ports::{FaceInfo, Result};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FitClass {
    Interference,
    Press,
    Snug,
    Slide,
    Loose,
}
#[derive(Debug, Clone, Serialize)]
pub struct CylinderFit {
    pub a_face: u32,
    pub b_face: u32,
    pub axis_point: [f64; 3],
    pub hole_d: f64,
    pub shaft_d: f64,
    pub clearance_mm: f64,
    pub class: FitClass,
}

/// Compare a hole and shaft where their finite axial intervals overlap.
pub fn cylinder_fits(a: &[FaceInfo], b: &[FaceInfo], clearance: &Clearance) -> Vec<CylinderFit> {
    let mut fits = vec![];
    for f in a {
        let Some(x) = &f.cylinder else { continue };
        for g in b {
            let Some(y) = &g.cylinder else { continue };
            if x.internal == y.internal {
                continue;
            }
            let (hole, shaft) = if x.internal { (x, y) } else { (y, x) };
            let Some(axis) = DVec3::from(hole.axis).try_normalize() else {
                continue;
            };
            let Some(along) = DVec3::from(shaft.axis).try_normalize() else {
                continue;
            };
            let cosine = axis.dot(along);
            if cosine.abs() < 0.1f64.to_radians().cos() {
                continue;
            }
            let base = DVec3::from(hole.start);
            let offset = DVec3::from(shaft.start) - base;
            let begin = offset.dot(axis);
            let end = begin + cosine * shaft.length;
            let lo = 0f64.max(begin.min(end));
            let hi = hole.length.min(begin.max(end));
            if hi <= lo {
                continue;
            }
            let centre = 0.5 * (lo + hi);
            if [lo, hi]
                .into_iter()
                .any(|t| (offset + along * ((t - begin) / cosine) - axis * t).length() > 0.01)
            {
                continue;
            }
            let hole_d = 2. * hole.radius;
            let shaft_d = 2. * shaft.radius;
            let gap = hole_d - shaft_d;
            let class = if gap < 0. {
                FitClass::Interference
            } else {
                [
                    (clearance.press, FitClass::Press),
                    (clearance.snug, FitClass::Snug),
                    (clearance.slide, FitClass::Slide),
                    (clearance.loose, FitClass::Loose),
                ]
                .into_iter()
                .min_by(|a, b| (a.0 - gap).abs().total_cmp(&(b.0 - gap).abs()))
                .unwrap()
                .1
            };
            fits.push(CylinderFit {
                a_face: f.index,
                b_face: g.index,
                axis_point: (base + axis * centre).into(),
                hole_d,
                shaft_d,
                clearance_mm: gap,
                class,
            });
        }
    }
    fits
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StackItem {
    pub nominal: f64,
    pub plus: f64,
    pub minus: f64,
    pub sign: i8,
}
#[derive(Debug, Clone, Serialize)]
pub struct Stack {
    pub nominal: f64,
    pub worst: [f64; 2],
    pub rss: [f64; 2],
}
pub fn stack(items: &[StackItem]) -> Stack {
    let (mut nominal, mut low, mut high) = (0., 0., 0.);
    let (mut rss_lo, mut rss_hi) = (0f64, 0f64);
    for s in items {
        nominal += s.nominal * f64::from(s.sign);
        let (down, up) = if s.sign < 0 {
            (s.plus, s.minus)
        } else {
            (s.minus, s.plus)
        };
        low += down;
        high += up;
        rss_lo = rss_lo.hypot(down);
        rss_hi = rss_hi.hypot(up);
    }
    Stack {
        nominal,
        worst: [nominal - low, nominal + high],
        rss: [nominal - rss_lo, nominal + rss_hi],
    }
}
pub fn checked_stack(items: &[StackItem]) -> Result<Stack> {
    if items.is_empty() {
        return Err("Add at least one dimension to the stack.".into());
    }
    for s in items {
        if !matches!(s.sign, -1 | 1)
            || !s.nominal.is_finite()
            || !s.plus.is_finite()
            || !s.minus.is_finite()
            || s.plus < 0.
            || s.minus < 0.
        {
            return Err(
                "Each dimension needs a finite nominal, nonnegative tolerances, and sign 1 or -1."
                    .into(),
            );
        }
    }
    let s = stack(items);
    if !s.worst.iter().chain(&s.rss).all(|v| v.is_finite()) {
        return Err("The tolerance stack exceeds the numeric range.".into());
    }
    Ok(s)
}
