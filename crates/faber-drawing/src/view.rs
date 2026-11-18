use crate::{ModelSource, clip, hatch, path, sheet};
use faber_model::drawing::{Direction, Drawing, Projection, Side, View, ViewKind};
use faber_ports::{
    Result,
    drawing::{Curve2, Projection2, ViewFrame},
    vector::{Path2, VectorDoc},
};
use glam::{DVec2, DVec3};
pub(crate) fn normal(f: &ViewFrame) -> DVec3 {
    DVec3::from(f.x).cross(f.y.into())
}
fn axes(x: DVec3, y: DVec3) -> ViewFrame {
    ViewFrame {
        origin: [0.; 3],
        x: x.to_array(),
        y: y.to_array(),
    }
}
/// The positive normal points from the model towards the viewer.
pub fn frame(d: &Direction) -> ViewFrame {
    let (x, y, z) = (DVec3::X, DVec3::Y, DVec3::Z);
    match d {
        Direction::Front => axes(x, z),
        Direction::Back => axes(-x, z),
        Direction::Right => axes(y, z),
        Direction::Left => axes(-y, z),
        Direction::Top => axes(x, y),
        Direction::Bottom => axes(x, -y),
        Direction::Iso => look(DVec3::new(-1., 1., -1.), z),
        Direction::Custom { view, up } => look((*view).into(), (*up).into()),
    }
}
fn look(direction: DVec3, up: DVec3) -> ViewFrame {
    let n = -direction.normalize();
    let x = up.cross(n).normalize();
    axes(x, n.cross(x))
}
pub(crate) fn projected(f: &ViewFrame, p: [f64; 3]) -> DVec2 {
    let d = DVec3::from(p) - DVec3::from(f.origin);
    DVec2::new(d.dot(f.x.into()), d.dot(f.y.into()))
}
pub(crate) fn turn(f: &ViewFrame, side: Side) -> ViewFrame {
    let (x, y, n) = (f.x.into(), f.y.into(), normal(f));
    let mut t = match side {
        Side::Top => axes(x, -n),
        Side::Bottom => axes(x, n),
        Side::Right => axes(-n, y),
        Side::Left => axes(n, y),
    };
    t.origin = f.origin;
    t
}
pub(crate) struct Layout {
    pub(crate) frame: ViewFrame,
    parts: Vec<String>,
    pub(crate) at: DVec2,
    pub(crate) scale: f64,
    centre: DVec2,
    extent: DVec2,
    paths: Vec<(&'static str, Path2)>,
}
impl Layout {
    pub(crate) fn sheet(&self, xy: DVec2) -> DVec2 {
        self.at + (xy - self.centre) * self.scale
    }
    pub(crate) fn size(&self) -> DVec2 {
        self.extent * self.scale
    }
    /// Take each witness point from the outline. When a side is straight,
    /// use the end nearest the dimension line.
    pub(crate) fn witnesses(&self, axis: usize, side: DVec2) -> Result<[DVec2; 2]> {
        let mut best: [Option<DVec2>; 2] = [None, None];
        let mut visit = |p: DVec2| {
            for (k, slot) in best.iter_mut().enumerate() {
                let sign = if k == 0 { -1. } else { 1. };
                if slot.is_none_or(|q| {
                    let diff = sign * (p[axis] - q[axis]);
                    diff > 1e-8 || (diff.abs() <= 1e-8 && side.dot(p) > side.dot(q))
                }) {
                    *slot = Some(p);
                }
            }
        };
        for (layer, path) in &self.paths {
            if !matches!(*layer, "VISIBLE" | "SECTION") {
                continue;
            }
            match path {
                Path2::Polyline { points, .. } => {
                    for &p in points {
                        visit(p.into());
                    }
                }
                Path2::Circle { centre, radius } => {
                    for dir in [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y] {
                        visit(DVec2::from(*centre) + dir * *radius);
                    }
                }
                Path2::Arc {
                    centre,
                    radius,
                    start_deg,
                    end_deg,
                } => {
                    let sweep = (end_deg - start_deg).rem_euclid(360.);
                    let point = |angle: f64| {
                        DVec2::from(*centre) + DVec2::from_angle(angle.to_radians()) * *radius
                    };
                    visit(point(*start_deg));
                    visit(point(*end_deg));
                    for angle in [0., 90., 180., 270.] {
                        if (angle - start_deg).rem_euclid(360.) <= sweep + 1e-9 {
                            visit(point(angle));
                        }
                    }
                }
            }
        }
        match best {
            [Some(a), Some(b)] => Ok([self.sheet(a), self.sheet(b)]),
            _ => Err("the view has no outline to measure.".into()),
        }
    }
    pub(crate) fn draw(&self, doc: &mut VectorDoc) -> Result<()> {
        for (l, p) in &self.paths {
            let p = match p {
                Path2::Polyline { points, closed } => {
                    path(points.iter().map(|&p| self.sheet(p.into())), *closed)
                }
                Path2::Circle { centre, radius } => Path2::Circle {
                    centre: self.sheet((*centre).into()).into(),
                    radius: radius * self.scale,
                },
                Path2::Arc {
                    centre,
                    radius,
                    start_deg,
                    end_deg,
                } => Path2::Arc {
                    centre: self.sheet((*centre).into()).into(),
                    radius: radius * self.scale,
                    start_deg: *start_deg,
                    end_deg: *end_deg,
                },
            };
            sheet::add(doc, l, p);
        }
        Ok(())
    }
    fn with_projection(&mut self, p: Projection2, hidden: bool) -> Result<()> {
        let a = DVec2::from(p.bounds[0]);
        let b = DVec2::from(p.bounds[1]);
        if !a.is_finite() || !b.is_finite() || a.cmpgt(b).any() {
            return Err("The view bounds are invalid.".into());
        }
        self.centre = (a + b) * 0.5;
        self.extent = b - a;
        self.paths
            .extend(p.visible.into_iter().map(|v| ("VISIBLE", curve(v))));
        if hidden {
            self.paths
                .extend(p.hidden.into_iter().map(|v| ("HIDDEN", curve(v))));
        }
        Ok(())
    }
}
fn curve(c: Curve2) -> Path2 {
    match c {
        Curve2::Line(points) => Path2::Polyline {
            points: points.to_vec(),
            closed: false,
        },
        Curve2::Polyline(points) => Path2::Polyline {
            points,
            closed: false,
        },
        Curve2::Circle { centre, radius } => Path2::Circle { centre, radius },
        Curve2::Arc {
            centre,
            radius,
            start_deg,
            end_deg,
        } => Path2::Arc {
            centre,
            radius,
            start_deg,
            end_deg,
        },
    }
}
pub(crate) fn build(
    v: &View,
    d: &Drawing,
    parent: Option<&Layout>,
    m: &dyn ModelSource,
) -> Result<Layout> {
    let mut l = Layout {
        frame: frame(&Direction::Front),
        parts: vec![],
        at: v.at.into(),
        scale: v
            .scale
            .unwrap_or_else(|| parent.map_or(d.scale, |p| p.scale)),
        centre: DVec2::ZERO,
        extent: DVec2::ZERO,
        paths: vec![],
    };
    match &v.kind {
        ViewKind::Base { parts, direction } => {
            l.frame = frame(direction);
            l.parts = parts.clone();
            l.with_projection(m.project(parts, &l.frame, v.hidden_lines)?, v.hidden_lines)?;
        }
        ViewKind::Projected { side, .. } => {
            let p = parent.unwrap();
            l.parts = p.parts.clone();
            l.frame = turn(&p.frame, *side);
            l.with_projection(
                m.project(&l.parts, &l.frame, v.hidden_lines)?,
                v.hidden_lines,
            )?;
            let i = usize::from(matches!(side, Side::Top | Side::Bottom));
            let sign = if matches!(side, Side::Top | Side::Right) {
                1.
            } else {
                -1.
            } * if d.projection == Projection::Third {
                1.
            } else {
                -1.
            };
            let offset = (l.at[i] - p.at[i]).abs();
            l.at = p.at;
            l.at[i] += sign
                * if offset > 1e-9 {
                    offset
                } else {
                    (l.size()[i] + p.size()[i]) * 0.5 + 12.
                };
        }
        ViewKind::Section { line, .. } => {
            let p = parent.unwrap();
            l.parts = p.parts.clone();
            let x = DVec3::from(p.frame.x);
            let y = DVec3::from(p.frame.y);
            let xy = DVec2::from(line[0]);
            let t = (DVec2::from(line[1]) - xy).normalize();
            let n = x * (-t.y) + y * t.x;
            let up = if n.dot(y).abs() > 0.999999 {
                normal(&p.frame)
            } else {
                y
            };
            l.frame = look(-n, up);
            l.frame.origin = (DVec3::from(p.frame.origin) + x * xy.x + y * xy.y).to_array();
            let back = m.cut_view(&l.parts, &l.frame, v.hidden_lines)?;
            let has_projection = !back.visible.is_empty() || !back.hidden.is_empty();
            l.with_projection(back, v.hidden_lines)?;
            let loops = m.section_in_frame(&l.parts, &l.frame)?;
            let mut lo = if has_projection {
                l.centre - l.extent / 2.
            } else {
                DVec2::splat(f64::INFINITY)
            };
            let mut hi = if has_projection {
                l.centre + l.extent / 2.
            } else {
                DVec2::splat(f64::NEG_INFINITY)
            };
            for ring in &loops {
                if ring.len() < 3 {
                    continue;
                }
                for &q in ring {
                    let q = q.into();
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
                l.paths.push((
                    "SECTION",
                    Path2::Polyline {
                        points: ring.clone(),
                        closed: true,
                    },
                ));
            }
            if lo.is_finite() && hi.is_finite() {
                l.centre = (lo + hi) * 0.5;
                l.extent = hi - lo;
            }
            l.paths.extend(
                hatch::lines(&loops, 3. / l.scale)?
                    .into_iter()
                    .map(|p| ("HATCH", p)),
            );
        }
        ViewKind::Detail { centre, radius, .. } => {
            let p = parent.unwrap();
            l.frame = p.frame;
            l.parts = p.parts.clone();
            l.centre = (*centre).into();
            l.extent = DVec2::splat(2. * radius);
            for (layer, source) in &p.paths {
                if *layer != "HIDDEN" || v.hidden_lines {
                    for clipped in clip::inside(source, l.centre, *radius) {
                        l.paths.push((layer, clipped));
                    }
                }
            }
            l.paths.push((
                "CUTTING",
                Path2::Circle {
                    centre: *centre,
                    radius: *radius,
                },
            ));
        }
    }
    Ok(l)
}
