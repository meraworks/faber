use crate::{
    ModelSource, Resolved, frame,
    view::{normal, projected, turn},
};
use faber_model::drawing::{
    Along, Anchor, Dimension, DimensionKind, Direction, Drawing, Projection, Sheet, Side, View,
    ViewKind,
};
use faber_ports::Result;
use glam::{DVec2, DVec3};
use std::collections::{BTreeMap, HashSet};

/// Reserve sheet space for automatic dimension labels on a 2 mm grid.
#[derive(Default)]
struct Labels(HashSet<(i32, i32)>);
impl Labels {
    fn place(&mut self, size: DVec2, width: f64, initial: f64, at: impl Fn(f64) -> DVec2) -> f64 {
        let pad = DVec2::new(width / 2. + 1., 2.);
        for lane in 0..48 {
            for sign in [1., -1.] {
                let offset = initial.signum() * (initial.abs() + lane as f64 * 4.5) * sign;
                let p = at(offset);
                let lo = p - pad;
                let hi = p + pad;
                if lo.cmplt(DVec2::splat(5.)).any()
                    || hi.cmpgt(size - DVec2::splat(5.)).any()
                    || (hi.x > size.x - 97. && lo.y < 20.)
                {
                    continue;
                }
                let cells = || {
                    ((lo.x / 2.).floor() as i32..=(hi.x / 2.).floor() as i32).flat_map(|x| {
                        ((lo.y / 2.).floor() as i32..=(hi.y / 2.).floor() as i32)
                            .map(move |y| (x, y))
                    })
                };
                if cells().all(|c| !self.0.contains(&c)) {
                    self.0.extend(cells());
                    return offset;
                }
            }
        }
        initial
    }
}

fn base(parts: &[String], direction: Direction) -> ViewKind {
    ViewKind::Base {
        parts: parts.to_vec(),
        direction,
    }
}
/// Place four views in two columns. Keep a band below them for the title.
pub fn auto_drawing(
    m: &dyn ModelSource,
    parts: &[String],
    sheet: Sheet,
    title: &str,
) -> Result<Drawing> {
    let bounds = m.bounds(parts)?;
    if bounds.iter().flatten().any(|v| !v.is_finite())
        || (0..3).any(|i| bounds[1][i] < bounds[0][i])
    {
        return Err("The model bounds are invalid.".into());
    }
    let size = DVec2::from(sheet.size_mm());
    let front_frame = frame(&Direction::Front);
    let mut projections = BTreeMap::new();
    // Use the same projections for layout and for export.
    for (name, f) in [
        ("front", front_frame),
        ("top", turn(&front_frame, Side::Top)),
        ("right", turn(&front_frame, Side::Right)),
        ("iso", frame(&Direction::Iso)),
    ] {
        let b = m.project(parts, &f, name != "iso")?.bounds;
        let lo = DVec2::from(b[0]);
        let hi = DVec2::from(b[1]);
        if !lo.is_finite() || !hi.is_finite() || lo.cmpgt(hi).any() {
            return Err("The view bounds are invalid.".into());
        }
        projections.insert(name, ((lo + hi) * 0.5, hi - lo));
    }
    let [front, top, right, iso] = ["front", "top", "right", "iso"].map(|id| projections[id].1);
    let col = [front.x.max(top.x), right.x.max(iso.x)];
    let row = [front.y.max(right.y), top.y.max(iso.y)];
    let needed = DVec2::new(col.iter().sum(), row.iter().sum());
    // Leave room on the left for the vertical labels and below for the title.
    let chars = crate::format_length(front.y)
        .len()
        .max(crate::format_length(top.y).len());
    let lower = DVec2::new(14. + chars as f64 * 1.9, 22.);
    let upper = DVec2::splat(6.);
    let footprint = |s: f64| needed * s + DVec2::splat(10.) + lower + upper;
    let scale = [10., 5., 2., 1., 0.5, 0.2, 0.1]
        .into_iter()
        .find(|&s| footprint(s).cmple(size).all())
        .ok_or_else(|| {
            format!(
                "The four views need {:.1} x {:.1} mm at scale 0.1. The sheet is {} x {} mm.",
                footprint(0.1).x,
                footprint(0.1).y,
                size.x,
                size.y
            )
        })?;
    let origin = lower + (size - footprint(scale)) * 0.5;
    let x0 = origin.x + col[0] * scale / 2.;
    let x1 = origin.x + col[0] * scale + 10. + col[1] * scale / 2.;
    let y0 = origin.y + row[0] * scale / 2.;
    let y1 = origin.y + row[0] * scale + 10. + row[1] * scale / 2.;
    let mut views = BTreeMap::new();
    for (name, at, kind) in [
        ("front", [x0, y0], base(parts, Direction::Front)),
        (
            "right",
            [x1, y0],
            ViewKind::Projected {
                from: "front".into(),
                side: Side::Right,
            },
        ),
        (
            "top",
            [x0, y1],
            ViewKind::Projected {
                from: "front".into(),
                side: Side::Top,
            },
        ),
        ("iso", [x1, y1], base(parts, Direction::Iso)),
    ] {
        views.insert(
            name.into(),
            View {
                at,
                scale: None,
                hidden_lines: name != "iso",
                label: None,
                kind,
            },
        );
    }
    let mut drawing = Drawing {
        title: title.into(),
        sheet,
        projection: Projection::Third,
        scale,
        views,
        dimensions: BTreeMap::new(),
        notes: vec![],
        title_block: BTreeMap::new(),
        parts_list: None,
    };
    let anchors = m.anchors(parts)?;
    let mut labels = Labels::default();
    let linear_offset = |labels: &mut Labels,
                         view: &str,
                         dir: Direction,
                         index: usize,
                         pa: [f64; 3],
                         pb: [f64; 3],
                         initial: f64| {
        let f = frame(&dir);
        let centre = projections[view].0;
        let xy = |p| DVec2::from(drawing.views[view].at) + (projected(&f, p) - centre) * scale;
        let (a, b) = (xy(pa), xy(pb));
        let n = if index == 0 { DVec2::Y } else { -DVec2::X };
        let width = crate::format_length(((a - b)[index] / scale).abs()).len() as f64 * 1.9;
        labels.place(size, width, initial, |offset| {
            let (start, end) = crate::dimension::baseline(a, b, n, offset);
            (start + end) * 0.5 + n * 1.5
        })
    };
    let vertices: Vec<_> = anchors
        .iter()
        .filter_map(|(a, r)| {
            if let Resolved::Point(p) = r {
                Some((a, *p))
            } else {
                None
            }
        })
        .collect();
    for (name, view, i, along, off) in [
        ("width", "front", 0, Along::Horizontal, -7.),
        ("height", "front", 1, Along::Vertical, 7.),
        ("depth", "top", 1, Along::Vertical, 7.),
    ] {
        let span = projections[view].1;
        if span[i] > 1e-7 {
            let width = crate::format_length(span[i]).len() as f64 * 1.9;
            let at = drawing.views[view].at.into();
            let offset = labels.place(size, width, off, |offset| {
                crate::dimension::overall_placement(at, span * scale, i, offset, width).1
            });
            drawing.dimensions.insert(
                name.into(),
                Dimension {
                    view: view.into(),
                    offset,
                    text: None,
                    kind: DimensionKind::Overall { along },
                },
            );
        }
    }
    let mut diameters: Vec<(&str, f64, Vec<Anchor>)> = vec![];
    for (i, (anchor, value)) in anchors.iter().enumerate() {
        let Resolved::Circle {
            centre,
            axis,
            radius,
        } = value
        else {
            continue;
        };
        let axis = DVec3::from(*axis).normalize_or_zero();
        let Some((view, dir)) = [
            ("front", Direction::Front),
            ("top", Direction::Top),
            ("right", Direction::Right),
            ("iso", Direction::Iso),
        ]
        .into_iter()
        .find(|(_, d)| normal(&frame(d)).dot(axis).abs() > 1. - 1e-6) else {
            continue;
        };
        if let Some(group) = diameters.iter_mut().find(|g| (g.1 - radius).abs() < 1e-6) {
            group.2.push(anchor.clone());
        } else {
            diameters.push((view, *radius, vec![anchor.clone()]));
        }
        if matches!(anchor,Anchor::CylinderAxis{face,..} if face.internal==Some(false)) {
            continue;
        }
        for (index, suffix, along) in [(0, "x", Along::Horizontal), (1, "y", Along::Vertical)] {
            let origin = vertices.iter().min_by(|a, b| {
                let ap = projected(&frame(&dir), a.1);
                let bp = projected(&frame(&dir), b.1);
                ap[index]
                    .total_cmp(&bp[index])
                    .then_with(|| ap[1 - index].total_cmp(&bp[1 - index]))
            });
            if let Some((start, point)) = origin {
                drawing.dimensions.insert(
                    format!("hole{}_{suffix}", i + 1),
                    Dimension {
                        view: view.into(),
                        offset: linear_offset(
                            &mut labels,
                            view,
                            dir,
                            index,
                            *point,
                            *centre,
                            if index == 0 { -5. } else { 5. },
                        ),
                        text: None,
                        kind: DimensionKind::Linear {
                            a: (*start).clone(),
                            b: anchor.clone(),
                            along,
                        },
                    },
                );
            }
        }
    }
    for (i, (view, _, mut edges)) in diameters.into_iter().enumerate() {
        let kind = if edges.len() == 1 {
            DimensionKind::Diameter {
                edge: edges.remove(0),
            }
        } else {
            DimensionKind::DiameterGroup { edges }
        };
        drawing.dimensions.insert(
            format!("diameter{}", i + 1),
            Dimension {
                view: view.into(),
                offset: 6.,
                text: None,
                kind,
            },
        );
    }
    Ok(drawing)
}
