//! The developed sheet in the coordinate system of the base wall.
mod curves;
use crate::build_for_flat;
use faber_model::{Flange, SheetRule, SketchProfile};
use faber_ports::vector::{Anchor, Layer, Path2, Text, VectorDoc};
use glam::{DAffine2, DVec2};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::{simplify::SimplifyShape, single::SingleFloatOverlay},
};

type Ring = Vec<[f64; 2]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
#[derive(Clone, Debug)]
pub struct BendLine {
    pub flange: String,
    pub line: [[f64; 2]; 2],
    pub angle_deg: f64,
    pub direction: Direction,
    pub radius: f64,
    pub allowance: f64,
}
#[derive(Clone, Debug)]
pub struct FlatPattern {
    pub outline: Vec<Ring>,
    pub holes: Vec<Ring>,
    pub bends: Vec<BendLine>,
    pub size_mm: [f64; 2],
    pub area_mm2: f64,
    pub cut_length_mm: f64,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    cut: Vec<Path2>,
    origin: [f64; 2],
}

#[derive(Clone, Copy)]
struct Bounds {
    low: DVec2,
    high: DVec2,
}
impl Bounds {
    fn of(pts: &[[f64; 2]]) -> Self {
        let mut low = DVec2::splat(f64::INFINITY);
        let mut high = -low;
        for &p in pts {
            low = low.min(p.into());
            high = high.max(p.into());
        }
        Self { low, high }
    }
    fn overlaps(&self, o: &Self) -> bool {
        self.high.cmpgt(o.low).all() && o.high.cmpgt(self.low).all()
    }
}
struct Piece<'a> {
    id: &'a str,
    bend: bool,
    loops: Vec<Ring>,
    bounds: Bounds,
}
fn area(p: &[[f64; 2]]) -> f64 {
    let origin = DVec2::from(p[0]);
    p.windows(2)
        .map(|v| (DVec2::from(v[0]) - origin).perp_dot(DVec2::from(v[1]) - origin))
        .sum::<f64>()
        / 2.
}
fn orientation(mut ring: Ring, outer: bool) -> Ring {
    if (area(&ring) > 0.) != outer {
        ring.reverse();
    }
    ring
}
fn perimeter(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(&a, &b)| DVec2::from(a).distance(b.into()))
        .sum()
}
fn placed(frame: DAffine2, ring: &[[f64; 2]]) -> Ring {
    ring.iter()
        .map(|&p| frame.transform_point2(p.into()).to_array())
        .collect()
}
fn overlaps(a: &Piece<'_>, b: &Piece<'_>) -> bool {
    if !a.bounds.overlaps(&b.bounds) {
        return false;
    }
    a.loops
        .overlay(&b.loops, OverlayRule::Intersect, FillRule::NonZero)
        .into_iter()
        .any(|s| s.first().is_some_and(|r| area(r).abs() > 1e-10))
}

pub fn flatten(
    (rule, base, flanges): (&SheetRule, &SketchProfile, &[Flange]),
) -> Result<FlatPattern, String> {
    develop(&build_for_flat(rule, base, flanges)?)
}

/// Develop the existing walls and bends in the plane of the base.
pub fn develop(tree: &crate::Folded) -> Result<FlatPattern, String> {
    let mut pieces = Vec::with_capacity(tree.walls.len() + tree.bends.len());

    for wall in &tree.walls {
        let outline = orientation(placed(wall.flat_frame, &wall.outline), true);
        let bounds = Bounds::of(&outline);
        let mut loops = vec![outline];
        loops.extend(
            wall.holes
                .iter()
                .map(|h| orientation(placed(wall.flat_frame, h), false)),
        );
        pieces.push(Piece {
            id: &wall.id,
            bend: false,
            loops,
            bounds,
        });
    }
    let mut bends = Vec::new();
    for bend in &tree.bends {
        if bend.allowance > 0. {
            let zone = orientation(bend.flat_zone.to_vec(), true);
            let bounds = Bounds::of(&zone);
            pieces.push(Piece {
                id: &bend.flange,
                bend: true,
                loops: vec![zone],
                bounds,
            });
        }
        bends.push(BendLine {
            flange: bend.flange.clone(),
            line: bend.flat_line,
            angle_deg: bend.angle_deg,
            direction: if bend.angle_deg >= 0. {
                Direction::Up
            } else {
                Direction::Down
            },
            radius: bend.radius,
            allowance: bend.allowance,
        });
    }
    let mut problems = std::collections::BTreeSet::new();
    for (i, a) in pieces.iter().enumerate() {
        for b in &pieces[i + 1..] {
            if !overlaps(a, b) {
                continue;
            }
            let (first, second) = if a.id <= b.id {
                (a.id, b.id)
            } else {
                (b.id, a.id)
            };
            let message = match (a.bend, b.bend) {
                (false, false) => {
                    format!("Walls {first} and {second} overlap in the flat pattern.")
                }
                (true, true) => format!("Bends {first} and {second} overlap in the flat pattern."),
                _ => {
                    let (wall, bend) = if a.bend { (b.id, a.id) } else { (a.id, b.id) };
                    format!("Wall {wall} and bend {bend} overlap in the flat pattern.")
                }
            };
            problems.insert(message);
        }
    }
    let low = pieces
        .iter()
        .fold(DVec2::splat(f64::INFINITY), |v, p| v.min(p.bounds.low));
    let high = pieces
        .iter()
        .fold(DVec2::splat(f64::NEG_INFINITY), |v, p| v.max(p.bounds.high));
    let mut sources = curves::Sources::new((high - low).max_element());
    for wall in &tree.walls {
        for profile in std::iter::once(&wall.profile.outer)
            .chain(&wall.profile.holes)
            .chain(&wall.reliefs)
        {
            sources.collect(profile, wall.flat_frame)?;
        }
    }
    let rings: Vec<_> = pieces.into_iter().flat_map(|p| p.loops).collect();
    let shapes = rings.simplify_shape(FillRule::NonZero);
    let mut outline = vec![];
    let mut holes = vec![];
    for shape in shapes {
        let mut rings = shape.into_iter();
        if let Some(r) = rings.next() {
            outline.push(r);
        }
        holes.extend(rings);
    }
    if outline.is_empty() {
        return Err("The flat pattern has no material.".into());
    }
    if outline.len() > 1 {
        problems.insert(format!(
            "The flat pattern has {} separate outlines.",
            outline.len()
        ));
    }
    let mut low = DVec2::splat(f64::INFINITY);
    let mut high = -low;
    let mut area_mm2 = 0.;
    let mut cut_length_mm = 0.;
    let mut cut = Vec::new();
    for (r, is_outer) in outline
        .iter()
        .map(|r| (r, true))
        .chain(holes.iter().map(|r| (r, false)))
    {
        let (a, length, bounds) = sources.trace(r, &mut cut);
        low = low.min(bounds.low);
        high = high.max(bounds.high);
        area_mm2 += if is_outer { a } else { -a };
        cut_length_mm += length;
    }
    Ok(FlatPattern {
        outline,
        holes,
        bends,
        size_mm: (high - low).to_array(),
        area_mm2,
        cut_length_mm,
        problems: problems.into_iter().collect(),
        warnings: tree.warnings.clone(),
        cut,
        origin: low.to_array(),
    })
}

/// Put the lower left corner at the sheet origin.
pub fn to_vector(p: &FlatPattern, title: &str) -> VectorDoc {
    let shift = |v: [f64; 2]| [v[0] - p.origin[0], v[1] - p.origin[1]];
    let layer = |name: &str, colour, dashed, paths| Layer {
        name: name.into(),
        colour,
        width_mm: 0.18,
        dashed,
        paths,
    };
    let cut = p
        .cut
        .iter()
        .map(|c| match c {
            Path2::Circle { centre, radius } => Path2::Circle {
                centre: shift(*centre),
                radius: *radius,
            },
            Path2::Polyline { points, closed } => Path2::Polyline {
                points: points.iter().map(|&v| shift(v)).collect(),
                closed: *closed,
            },
            Path2::Arc {
                centre,
                radius,
                start_deg,
                end_deg,
            } => Path2::Arc {
                centre: shift(*centre),
                radius: *radius,
                start_deg: *start_deg,
                end_deg: *end_deg,
            },
        })
        .collect();
    let bend = p
        .bends
        .iter()
        .map(|b| Path2::Polyline {
            points: b.line.map(shift).to_vec(),
            closed: false,
        })
        .collect();
    let height = (p.size_mm[1] * 0.04).clamp(0.5, 3.);
    let texts = if title.is_empty() {
        vec![]
    } else {
        vec![Text {
            at: [height, height],
            height_mm: height,
            text: title.into(),
            anchor: Anchor::Start,
            layer: "TEXT".into(),
        }]
    };
    VectorDoc {
        size_mm: p.size_mm,
        layers: vec![
            layer("CUT", [0, 0, 0], false, cut),
            layer("BEND", [40, 92, 145], true, bend),
            layer("TEXT", [50, 50, 50], false, vec![]),
        ],
        texts,
    }
}
