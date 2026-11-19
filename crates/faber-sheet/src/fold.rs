use crate::{Bend, Folded, Wall, tree};
use faber_model::{Flange, Relief, SheetRule, SketchLoop, SketchProfile};
use glam::{DAffine2, DAffine3, DMat2, DMat3, DQuat, DVec2, DVec3};
use std::collections::{BTreeMap, VecDeque};
const EPS: f64 = 1e-7;

#[derive(Clone)]
struct Attachment {
    parent: usize,
    child: usize,
    edges: [DVec2; 2],
    angle: f64,
}
fn positive(v: f64) -> bool {
    (1e-6..=1e6).contains(&v)
}
fn parameters(rule: &SheetRule) -> Result<f64, String> {
    if !positive(rule.thickness) || !positive(rule.bend_radius) {
        return Err(
            "Sheet thickness and bend radius must be between 0.000001 and 1000000 mm.".into(),
        );
    }
    let k = rule.k_factor.unwrap_or(0.44);
    if !k.is_finite() || !(0.0..=1.0).contains(&k) {
        return Err("The K factor must be between zero and one.".into());
    }
    match rule.relief {
        Relief::Rectangular { width, depth } if !positive(width) || !positive(depth) => {
            return Err("Relief width and depth must be greater than zero.".into());
        }
        Relief::Round { diameter } if !positive(diameter) => {
            return Err("The relief diameter must be greater than zero.".into());
        }
        _ => {}
    }
    Ok(k)
}
fn tree_order(flanges: &[Flange]) -> Result<Vec<&Flange>, String> {
    if flanges.len() > 1024 {
        return Err("A sheet can have up to 1024 flanges.".into());
    }
    let mut ids = BTreeMap::new();
    let mut children = BTreeMap::<&str, Vec<usize>>::new();
    for (i, f) in flanges.iter().enumerate() {
        if f.id == "base" || f.id.is_empty() || ids.insert(f.id.as_str(), i).is_some() {
            return Err(format!("Invalid or repeated flange id: {}.", f.id));
        }
        children.entry(&f.on).or_default().push(i);
    }
    for f in flanges {
        if f.on != "base" && !ids.contains_key(f.on.as_str()) {
            return Err(format!("Unknown wall {} for flange {}.", f.on, f.id));
        }
    }
    let mut q = VecDeque::from(["base"]);
    let mut ordered = Vec::with_capacity(flanges.len());
    while let Some(parent) = q.pop_front() {
        if let Some(indices) = children.get(parent) {
            for &i in indices {
                ordered.push(&flanges[i]);
                q.push_back(&flanges[i].id);
            }
        }
    }
    if ordered.len() != flanges.len() {
        return Err("The flanges form a cycle.".into());
    }
    Ok(ordered)
}
fn cut_at(
    parent: &mut Wall,
    endpoint: DVec2,
    away: DVec2,
    inward: DVec2,
    kind: &Relief,
    count: &mut usize,
) -> Result<(), String> {
    let cut = match *kind {
        Relief::None => return Ok(()),
        Relief::Rectangular { width, depth } => SketchLoop::Polygon {
            points: [
                endpoint,
                endpoint + away * width,
                endpoint + away * width + inward * depth,
                endpoint + inward * depth,
            ]
            .map(|p| p.to_array())
            .to_vec(),
            corner_radius: None,
        },
        Relief::Round { diameter } => SketchLoop::Circle {
            centre: (endpoint + away * (diameter / 2.)).to_array(),
            radius: diameter / 2.,
        },
    };
    tree::relief(parent, cut, count)
}
fn hole_warning(
    w: &Wall,
    a: DVec2,
    b: DVec2,
    threshold: f64,
    bend: &str,
    warnings: &mut Vec<String>,
) {
    for (i, h) in w.holes.iter().enumerate() {
        let d = h
            .iter()
            .zip(h.iter().cycle().skip(1))
            .map(|(&p, &q)| {
                let (p, q) = (DVec2::from(p), DVec2::from(q));
                tree::distance_to_segment(p, a, b)
                    .min(tree::distance_to_segment(a, p, q))
                    .min(tree::distance_to_segment(b, p, q))
            })
            .fold(f64::INFINITY, f64::min);
        if d + EPS < threshold {
            warnings.push(format!(
                "Wall {} hole {} is {d:.3} mm from bend {bend}. Allow {threshold:.3} mm.",
                w.id,
                i + 1
            ));
        }
    }
}
pub(crate) fn build(
    rule: &SheetRule,
    base: &SketchProfile,
    flanges: &[Flange],
    check: bool,
) -> Result<Folded, String> {
    let k = parameters(rule)?;
    let base = tree::wall("base", base.clone(), DAffine3::IDENTITY, DAffine2::IDENTITY)?;
    let mut used = tree::wall_samples(&base);
    let mut walls = vec![base];
    let mut bends = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = BTreeMap::from([("base", 0usize)]);
    let mut attachments = Vec::<Attachment>::new();
    let (t, r) = (rule.thickness, rule.bend_radius);
    if r < t && !flanges.is_empty() {
        warnings.push(format!(
            "The bend radius is {r:.3} mm. It is less than the sheet thickness of {t:.3} mm."
        ));
    }
    for f in tree_order(flanges)? {
        let parent_index = ids[f.on.as_str()];
        let child_index = walls.len();
        let parent = &mut walls[parent_index];
        if f.edge == 0 || f.edge as usize > parent.edge_count {
            return Err(format!("Unknown edge {} on wall {}.", f.edge, parent.id));
        }
        if parent_index != 0 && f.edge == 1 {
            return Err(format!(
                "Edge 1 of wall {} already joins its parent bend.",
                parent.id
            ));
        }
        let (_, ends) = parent
            .straight_edges
            .iter()
            .find(|(i, _)| *i == f.edge)
            .ok_or_else(|| {
                format!(
                    "Flange {} needs a straight edge on wall {}.",
                    f.id, parent.id
                )
            })?;
        let [a, b] = ends.map(DVec2::from);
        let direction = (b - a).normalize();
        let length = a.distance(b);
        let s = f.inset_start.unwrap_or(0.);
        let e = f.inset_end.unwrap_or(0.);
        let width = length - s - e;
        if !s.is_finite()
            || !e.is_finite()
            || s < 0.
            || e < 0.
            || width <= EPS
            || !positive(f.length)
        {
            return Err(format!("Check the length and insets of flange {}.", f.id));
        }
        let angle = f.angle_deg.unwrap_or(90.);
        if !angle.is_finite() || angle.abs() > 180. {
            return Err(format!(
                "The bend angle for {} must be between -180 and 180 degrees.",
                f.id
            ));
        }
        let outward = if parent.ccw {
            -direction.perp()
        } else {
            direction.perp()
        };
        let start = a + direction * s;
        let end = b - direction * e;
        let (frame, flat_frame, bend) =
            bend_frames(parent, f, rule, k, [start, end], outward, angle);
        let child = tree::wall(
            &f.id,
            SketchProfile {
                outer: SketchLoop::Rectangle {
                    origin: [0.; 2],
                    size: [width, f.length],
                },
                holes: f.holes.clone(),
            },
            frame,
            flat_frame,
        )?;
        tree::charge_samples(&mut used, tree::wall_samples(&child))?;
        if angle != 0. {
            hole_warning(parent, start, end, r + 2. * t, &f.id, &mut warnings);
            hole_warning(
                &child,
                DVec2::ZERO,
                DVec2::X * width,
                r + 2. * t,
                &f.id,
                &mut warnings,
            );
        }
        if s > EPS {
            cut_at(parent, start, -direction, -outward, &rule.relief, &mut used)?;
        }
        if e > EPS {
            cut_at(parent, end, direction, -outward, &rule.relief, &mut used)?;
        }
        bends.push(bend);
        attachments.push(Attachment {
            parent: parent_index,
            child: child_index,
            edges: [start, end],
            angle,
        });
        ids.insert(&f.id, child_index);
        walls.push(child);
    }
    corner_reliefs(&mut walls, &attachments, &rule.relief, check, &mut used)?;
    if check {
        crate::collision::check(&walls, &bends, t)?;
    }
    Ok(Folded {
        walls,
        bends,
        warnings,
    })
}

fn corner_reliefs(
    walls: &mut [Wall],
    attachments: &[Attachment],
    relief: &Relief,
    check: bool,
    count: &mut usize,
) -> Result<(), String> {
    for (i, a) in attachments.iter().enumerate() {
        for b in &attachments[..i] {
            if a.parent != b.parent || a.angle * b.angle <= 0. {
                continue;
            }
            let Some((start, end)) = a.edges.iter().enumerate().find_map(|(start, p)| {
                b.edges
                    .iter()
                    .position(|q| p.distance(*q) < EPS)
                    .map(|end| (start, end))
            }) else {
                continue;
            };
            let corner = a.edges[start];
            let along = (a.edges[1 - start] - corner).normalize();
            let other = (b.edges[1 - end] - corner).normalize();
            if along.perp_dot(other).abs() < 1e-8 {
                continue;
            }
            let cut = match *relief {
                Relief::None if check => {
                    return Err(format!(
                        "Flanges {} and {} overlap at the corner. Add an inset or a relief.",
                        walls[b.child].id, walls[a.child].id
                    ));
                }
                Relief::None => continue,
                Relief::Round { diameter } => SketchLoop::Circle {
                    centre: corner.to_array(),
                    radius: diameter / 2.,
                },
                Relief::Rectangular { width, depth } => {
                    let inward = along.perp() * along.perp_dot(other).signum();
                    SketchLoop::Polygon {
                        points: [
                            corner,
                            corner + along * width,
                            corner + along * width + inward * depth,
                            corner + inward * depth,
                        ]
                        .map(|p| p.to_array())
                        .to_vec(),
                        corner_radius: None,
                    }
                }
            };
            tree::relief(&mut walls[a.parent], cut, count)?;
        }
    }
    Ok(())
}

fn bend_frames(
    parent: &Wall,
    f: &Flange,
    rule: &SheetRule,
    k: f64,
    [start, end]: [DVec2; 2],
    outward: DVec2,
    angle: f64,
) -> (DAffine3, DAffine2, Bend) {
    let (r, t) = (rule.bend_radius, rule.thickness);
    let width = start.distance(end);
    let x2 = -outward.perp();
    let origin = if parent.ccw { end } else { start };
    let pos = |p: DVec2| parent.frame.transform_point3(p.extend(0.));
    let x = parent.frame.transform_vector3(x2.extend(0.));
    let out = parent.frame.transform_vector3(outward.extend(0.));
    let n = parent.frame.transform_vector3(DVec3::Z);
    let foot = pos(origin);
    let theta = angle.to_radians();
    let allowance = theta.abs() * (r + k * t);
    let hub = foot + n * if angle >= 0. { t + r } else { -r };
    let rotation = DQuat::from_axis_angle(x, theta);
    let frame = DAffine3::from_mat3_translation(
        DMat3::from_cols(x, rotation * out, rotation * n),
        hub + rotation * (foot - hub),
    );
    let flat = parent.flat_frame;
    let joint = flat.transform_point2(origin);
    let fx = flat.transform_vector2(x2);
    let fy = flat.transform_vector2(outward);
    let far = joint + fy * allowance;
    let flat_frame = DAffine2::from_mat2_translation(DMat2::from_cols(fx, fy), far);
    let section = [
        foot,
        foot + x * width,
        foot + x * width + n * t,
        foot + n * t,
    ]
    .map(|p| p.to_array());
    let line = [hub, hub + x * width].map(|p| p.to_array());
    let flat_zone = [joint, joint + fx * width, far + fx * width, far].map(|p| p.to_array());
    let flat_line = [
        joint + fy * (allowance / 2.),
        joint + fx * width + fy * (allowance / 2.),
    ]
    .map(|p| p.to_array());
    let bend = Bend {
        flange: f.id.clone(),
        line,
        section,
        angle_deg: angle,
        radius: r,
        allowance,
        width,
        flat_line,
        flat_zone,
    };
    (frame, flat_frame, bend)
}
