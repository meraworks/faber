use crate::{
    ModelSource, Resolved, sheet,
    view::{Layout, normal, projected},
};
use faber_model::drawing::{Along, Dimension, DimensionKind};
use faber_ports::{
    Result,
    vector::{Path2, VectorDoc},
};
use glam::{DVec2, DVec3};

pub fn format_length(mm: f64) -> String {
    let s = format!("{:.2}", if mm.abs() < 0.005 { 0. } else { mm });
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
fn point(r: Resolved) -> [f64; 3] {
    match r {
        Resolved::Point(p) => p,
        Resolved::Circle { centre, .. } => centre,
        Resolved::Line([a, b]) => ((DVec3::from(a) + DVec3::from(b)) * 0.5).into(),
    }
}
fn label(d: &Dimension, value: String) -> String {
    d.text
        .as_ref()
        .map_or_else(|| value.clone(), |s| s.replace("<>", &value))
}
pub(crate) fn baseline(a: DVec2, b: DVec2, n: DVec2, offset: f64) -> (DVec2, DVec2) {
    let levels = [a.dot(n), b.dot(n)];
    let level = if offset >= 0. {
        levels[0].max(levels[1])
    } else {
        levels[0].min(levels[1])
    } + offset;
    (a + n * (level - levels[0]), b + n * (level - levels[1]))
}
const L: &str = "DIMENSION";
fn extension_lines(doc: &mut VectorDoc, from: [DVec2; 2], to: [DVec2; 2], past: DVec2) {
    let [a, b] = to;
    sheet::line(doc, L, from[0], a + past);
    sheet::line(doc, L, from[1], b + past);
    sheet::line(doc, L, a, b);
    let dir = (b - a).normalize_or_zero();
    sheet::arrow(doc, L, a, -dir);
    sheet::arrow(doc, L, b, dir);
}
/// Return the baseline centre and label position outside the silhouette.
pub(crate) fn overall_placement(
    at: DVec2,
    size: DVec2,
    index: usize,
    offset: f64,
    text_width: f64,
) -> (DVec2, DVec2, DVec2) {
    let side = if index == 0 { DVec2::Y } else { -DVec2::X } * if offset < 0. { -1. } else { 1. };
    let line = at + side * (size[1 - index] / 2. + offset.abs().max(5.));
    let text = line
        + side
            * if index == 0 {
                3.2
            } else {
                text_width / 2. + 1.
            };
    (line, text, side)
}
pub(crate) fn draw(
    doc: &mut VectorDoc,
    d: &Dimension,
    v: &Layout,
    m: &dyn ModelSource,
    obstacles: &[(DVec2, DVec2)],
) -> Result<()> {
    let xy = |p| projected(&v.frame, p);
    match &d.kind {
        DimensionKind::Linear { a, b, along } => {
            let (p, q) = (xy(point(m.resolve(a)?)), xy(point(m.resolve(b)?)));
            let (a, b) = (v.sheet(p), v.sheet(q));
            let axis = match along {
                Along::Horizontal => DVec2::X,
                Along::Vertical => DVec2::Y,
                Along::Aligned => (q - p).normalize_or_zero(),
            };
            let axis = if axis == DVec2::ZERO { DVec2::X } else { axis };
            let n = axis.perp();
            let (a1, b1) = baseline(a, b, n, d.offset);
            let extra = n * d.offset.signum() * 1.5;
            extension_lines(doc, [a, b], [a1, b1], extra);
            sheet::text(
                doc,
                L,
                (a1 + b1) * 0.5 + n * 1.5,
                label(d, format_length((q - p).dot(axis).abs())),
                3.,
            );
        }
        DimensionKind::Overall { along } => {
            let i = match along {
                Along::Horizontal => 0,
                Along::Vertical => 1,
                Along::Aligned => {
                    return Err(
                        "an overall dimension needs a horizontal or vertical direction.".into(),
                    );
                }
            };
            let value = label(d, format_length(v.size()[i] / v.scale));
            let (line, text, side) = overall_placement(
                v.at,
                v.size(),
                i,
                d.offset,
                value.chars().count() as f64 * 1.9,
            );
            let witness = v.witnesses(i, side)?;
            let to = witness.map(|p| p + side * ((line - p).dot(side)));
            extension_lines(doc, witness, to, side * 1.5);
            sheet::text(doc, L, text, value, 3.);
        }
        DimensionKind::Diameter { .. }
        | DimensionKind::Radius { .. }
        | DimensionKind::DiameterGroup { .. } => {
            let edges = match &d.kind {
                DimensionKind::DiameterGroup { edges } => edges.as_slice(),
                DimensionKind::Diameter { edge } | DimensionKind::Radius { edge } => {
                    std::slice::from_ref(edge)
                }
                _ => unreachable!(),
            };
            let mut measured: Option<f64> = None;
            let mut representative = None;
            for edge in edges {
                let Resolved::Circle {
                    centre,
                    axis,
                    radius,
                } = m.resolve(edge)?
                else {
                    return Err("the reference is not circular.".into());
                };
                if measured.is_some_and(|r| (r - radius).abs() > 1e-6) {
                    return Err("the diameters in the group no longer match.".into());
                }
                measured.get_or_insert(radius);
                if normal(&v.frame)
                    .dot(DVec3::from(axis).normalize_or_zero())
                    .abs()
                    >= 1. - 1e-6
                {
                    representative.get_or_insert((centre, radius));
                }
            }
            let (centre, radius) =
                representative.ok_or("the circle is not parallel to the view.")?;
            let centre = v.sheet(xy(centre));
            if edges.len() > 1 {
                let text = label(
                    d,
                    format!("{}x Ø{}", edges.len(), format_length(2. * radius)),
                );
                return crate::leader::group(
                    doc,
                    v,
                    obstacles,
                    centre,
                    radius * v.scale,
                    d.offset,
                    text,
                );
            }
            let dir = DVec2::from_angle(35f64.to_radians());
            let rad = radius * v.scale;
            let tip = centre + dir * rad;
            let dia = !matches!(d.kind, DimensionKind::Radius { .. });
            let start = if dia { centre - dir * rad } else { centre };
            let end = tip + dir * d.offset.abs().max(5.);
            sheet::line(doc, L, start, end);
            sheet::arrow(doc, L, tip, dir);
            if dia {
                sheet::arrow(doc, L, start, -dir);
            }
            let s = if dia {
                format!("Ø{}", format_length(2. * radius))
            } else {
                format!("R{}", format_length(radius))
            };
            sheet::text(doc, L, end + DVec2::Y * 1.5, label(d, s), 3.);
        }
        DimensionKind::Angle { a, b } => {
            let endpoints = |value| {
                if let Resolved::Line([a, b]) = value {
                    Ok((v.sheet(xy(a)), v.sheet(xy(b))))
                } else {
                    Err("the angle needs two straight edges.".to_owned())
                }
            };
            let (p, q) = endpoints(m.resolve(a)?)?;
            let (r, s) = endpoints(m.resolve(b)?)?;
            let mut u = (q - p).normalize_or_zero();
            let mut w = (s - r).normalize_or_zero();
            let cross = u.perp_dot(w);
            if cross.abs() < 1e-10 {
                return Err("the angle has no projected intersection.".into());
            }
            let centre = p + u * ((r - p).perp_dot(w) / cross);
            if ((p + q) * 0.5 - centre).dot(u) < 0. {
                u = -u;
            }
            if ((r + s) * 0.5 - centre).dot(w) < 0. {
                w = -w;
            }
            if u.perp_dot(w) < 0. {
                std::mem::swap(&mut u, &mut w);
            }
            let angle = u.perp_dot(w).atan2(u.dot(w));
            let start = u.y.atan2(u.x);
            let radius = d.offset.abs().max(3.);
            sheet::add(
                doc,
                L,
                Path2::Arc {
                    centre: centre.into(),
                    radius,
                    start_deg: start.to_degrees(),
                    end_deg: (start + angle).to_degrees(),
                },
            );
            for (ray, arrow) in [(u, -u.perp()), (w, w.perp())] {
                let tip = centre + ray * radius;
                sheet::line(doc, L, centre, tip + ray * 1.5);
                sheet::arrow(doc, L, tip, arrow);
            }
            sheet::text(
                doc,
                L,
                centre + DVec2::from_angle(start + angle / 2.) * (radius + 2.),
                label(d, format!("{}°", format_length(angle.to_degrees()))),
                3.,
            );
        }
    }
    Ok(())
}
