use crate::{sheet, view::Layout};
use faber_ports::{
    Result,
    vector::{Anchor, VectorDoc},
};
use glam::DVec2;

/// Choose the shortest leader whose text has free space on the sheet.
pub(crate) fn group(
    doc: &mut VectorDoc,
    v: &Layout,
    obstacles: &[(DVec2, DVec2)],
    centre: DVec2,
    radius: f64,
    offset: f64,
    label: String,
) -> Result<()> {
    let half = DVec2::new(label.chars().count() as f64 * 1.18 + 1., 3.5);
    let mut blocked = obstacles.to_vec();
    for t in &doc.texts {
        let w = t.height_mm * t.text.chars().count() as f64 * 0.39;
        let dx = match t.anchor {
            Anchor::Start => w,
            Anchor::End => -w,
            Anchor::Centre => 0.,
        };
        blocked.push((
            DVec2::from(t.at) + DVec2::new(dx, t.height_mm / 3.),
            DVec2::new(w + 1., t.height_mm * 0.75 + 1.),
        ));
    }
    let mut best = None;
    for degrees in [
        35., 145., -35., -145., 60., 120., -60., -120., 15., 165., -15., -165., 80., 100., -80.,
        -100.,
    ] {
        let dir = DVec2::from_angle(f64::to_radians(degrees));
        let side = dir.signum();
        let boundary = v.at + side * (v.size() / 2. + DVec2::splat(2.));
        let exit = ((boundary - centre) / dir)
            .min_element()
            .max(radius + offset.abs().max(4.));
        for extra in [0., 4., 8., 16., 32., 64.] {
            let distance = exit + extra;
            let elbow = centre + distance * dir;
            let shoulder = elbow + DVec2::X * (side.x * 3.);
            let at = shoulder + DVec2::new(side.x * half.x, 0.);
            let lo = at - half;
            let hi = at + half;
            if lo.cmplt(DVec2::splat(5.)).any()
                || hi.cmpgt(DVec2::from(doc.size_mm) - DVec2::splat(5.)).any()
            {
                continue;
            }
            if blocked
                .iter()
                .any(|(p, h)| ((at - *p).abs() - *h - half).max_element() < 1.)
            {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|(length, _, _, _, _)| distance < *length)
            {
                best = Some((distance, dir, elbow, shoulder, at));
            }
            break;
        }
    }
    let (_, dir, elbow, shoulder, at) =
        best.ok_or("the sheet has no space for the grouped diameter label.")?;
    let start = centre - dir * radius;
    let tip = centre + dir * radius;
    sheet::line(doc, "DIMENSION", start, elbow);
    sheet::line(doc, "DIMENSION", elbow, shoulder);
    sheet::arrow(doc, "DIMENSION", start, -dir);
    sheet::arrow(doc, "DIMENSION", tip, dir);
    sheet::text(doc, "DIMENSION", at, label, 3.);
    Ok(())
}
